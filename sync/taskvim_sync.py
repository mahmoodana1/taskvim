#!/usr/bin/env python3
"""taskvim-sync: keep taskvim's data.json whole across machines.

Syncthing copies data.json as one file. When two machines change it while
they cannot see each other, one version wins and the other becomes a
`data.sync-conflict-*.json` copy that nobody reads. This service runs on every
machine and fixes that, whatever program wrote the file (taskvim, the
subscreen agent, dojo, a script):

* stamp: every record (any top-level list of objects with an `id`) carries
  `_m`, when it last changed (ms), and `_c`, a birth id derived from the
  collection, id and name, so the same task gets the same `_c` everywhere and
  two different tasks that grabbed the same counter id do not. Deletions go to
  `_sync.del` as tombstones, other top-level values get a time in `_sync.m`.
  Stamps come from diffing the file against the last version this service saw,
  so writers need to know nothing about them.
* merge: a conflict copy is folded back in record by record: the newer edit
  wins, deletions stick, a task created on both sides under one id keeps both
  (the incoming one is renumbered). The copy then moves to the state dir.

Standard library only. `--once` does one pass, `--test` runs the self-tests.
"""

import copy
import glob
import hashlib
import json
import os
import shutil
import sys
import time

DATA_DIR = os.path.join(os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share"), "taskvim")
STATE_DIR = os.path.join(os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state"), "taskvim-sync")
META = ("_m", "_c")
KEEP_TOMBSTONES_MS = 120 * 86400 * 1000
HISTORY_MS, HISTORY_MIN = 45 * 86400 * 1000, 50
# Counters only ever grow; taking the larger can never hand out a used id.
COUNTERS = ("next_id", "next_color", "version")
# Who points at whom, for renumbering a record that collided on its id.
REFS = {
    "projects": [("tasks", "project"), ("sections", "project"), ("log", "project")],
    "sections": [("tasks", "section"), ("log", "section")],
    "tasks": [("log", "task"), ("dashboard", None)],
}


def now_ms():
    return int(time.time() * 1000)


def log(msg):
    print(time.strftime("%H:%M:%S"), msg, flush=True)


# ---------- records ----------

def is_records(v):
    return isinstance(v, list) and bool(v) and all(isinstance(r, dict) and "id" in r for r in v)


def collections(d):
    return [k for k, v in d.items() if k != "_sync" and is_records(v)]


def content(r):
    return {k: v for k, v in r.items() if k not in META}


def birth(coll, r):
    name = next((r[k] for k in ("text", "name", "title", "date") if isinstance(r.get(k), str)), "")
    return hashlib.sha1(f"{coll}\0{r['id']}\0{name}".encode()).hexdigest()[:12]


def index(d, coll):
    return {r["id"]: r for r in d.get(coll) or [] if isinstance(r, dict) and "id" in r}


def sync_meta(d):
    s = d.get("_sync")
    if not isinstance(s, dict):
        s = d["_sync"] = {}
    s.setdefault("del", {})
    s.setdefault("m", {})
    return s


# ---------- stamping ----------

def stamp(cur, base, now=None):
    """Return `cur` with stamps for everything that changed since `base`."""
    now = now_ms() if now is None else now
    out = copy.deepcopy(cur)
    base = base or {}
    bsync = base.get("_sync") if isinstance(base.get("_sync"), dict) else {}
    s = sync_meta(out)
    # A writer that drops unknown keys (an old taskvim) drops _sync too.
    for k in ("del", "m"):
        for key, ts in (bsync.get(k) or {}).items():
            s[k].setdefault(key, ts)

    for k in base:
        # Writers emit every key they know, so one that vanished was dropped
        # by an older build that does not know it: put it back.
        out.setdefault(k, copy.deepcopy(base[k]))
    colls = set(collections(out)) | set(collections(base))
    for coll in colls:
        bidx = index(base, coll)
        for r in out.get(coll) or []:
            if not isinstance(r, dict) or "id" not in r:
                continue
            b = bidx.get(r["id"])
            for k in (b or {}):
                # Writers emit every field they know, so a key that vanished
                # was dropped by an older build that does not know it.
                r.setdefault(k, b[k])
            if "_c" not in r:
                r["_c"] = b.get("_c", birth(coll, b)) if b else birth(coll, r)
            if b is not None and content(b) == content(r):
                r["_m"] = max(r.get("_m", 0), b.get("_m", 0))
            elif "_m" in r and (b is None or r["_m"] != b.get("_m")):
                pass  # already stamped elsewhere (it came through Syncthing)
            else:
                r["_m"] = now
        live = {(r.get("id"), r.get("_c")) for r in out.get(coll) or [] if isinstance(r, dict)}
        for b in bidx.values():
            c = b.get("_c", birth(coll, b))
            if (b["id"], c) not in live:
                s["del"].setdefault(f"{coll}/{b['id']}/{c}", now)

    for k in set(out) | set(base):
        if k == "_sync" or k in colls:
            continue
        if out.get(k) != base.get(k) and s["m"].get(k) == bsync.get("m", {}).get(k):
            s["m"][k] = now
    s["del"] = {k: t for k, t in s["del"].items() if now - t < KEEP_TOMBSTONES_MS}
    return out


# ---------- merging ----------

def renumber_collisions(ours, theirs, base=None):
    """Give each incoming record whose id belongs to a different record here a fresh id.
    An id already in the common ancestor is one record on both sides, never a collision."""
    theirs = copy.deepcopy(theirs)
    base = base or {}
    next_id = max([ours.get("next_id", 1), theirs.get("next_id", 1)] +
                  [r["id"] + 1 for d in (ours, theirs) for c in collections(d)
                   for r in d[c] if isinstance(r.get("id"), int)])
    order = [c for c in ("projects", "sections", "tasks") if c in REFS] + \
            [c for c in collections(theirs) if c not in REFS]
    for coll in order:
        oidx, bidx = index(ours, coll), index(base, coll)
        moved = {}
        for r in theirs.get(coll) or []:
            o = oidx.get(r.get("id"))
            if o is None or not isinstance(r["id"], int) or r["id"] in bidx:
                continue
            if o.get("_c", birth(coll, o)) != r.get("_c", birth(coll, r)):
                moved[r["id"]] = next_id
                r["id"] = next_id
                next_id += 1
        for old, new in moved.items():
            log(f"{coll} {old} existed on both sides as different records; incoming one is now {new}")
            for rcoll, field in REFS.get(coll, []):
                if field is None:
                    theirs[rcoll] = [new if x == old else x for x in theirs.get(rcoll) or []]
                    continue
                for x in theirs.get(rcoll) or []:
                    if isinstance(x, dict) and x.get(field) == old:
                        x[field] = new
    theirs["next_id"] = max(next_id, theirs.get("next_id", 0))
    return theirs


def merge(ours, theirs, ours_mtime=0, theirs_mtime=0, base=None):
    """Fold two versions together.

    Records both sides stamped go by their stamps. Where a side has no real
    stamp (`_m` missing or 0: a machine without this service, or a record not
    touched since it was installed) and `base`, a common ancestor, is known,
    it is a three-way merge: the side that still matches the ancestor did not
    change it. Otherwise an unstamped edit counts as made at its file's mtime.
    """
    base = base or {}
    theirs = copy.deepcopy(theirs)
    for coll in collections(theirs):
        bi = index(base, coll)
        for r in theirs[coll]:
            if "_c" not in r and r["id"] in bi:
                # unstamped but already there in the ancestor: the same record, maybe renamed
                r["_c"] = bi[r["id"]].get("_c", birth(coll, bi[r["id"]]))
    theirs = renumber_collisions(ours, theirs, base)
    out = copy.deepcopy(ours)
    so, st = sync_meta(copy.deepcopy(ours)), sync_meta(copy.deepcopy(theirs))
    dels = dict(so["del"])
    for k, t in st["del"].items():
        dels[k] = max(t, dels.get(k, 0))

    colls = set(collections(ours)) | set(collections(theirs))
    born = {"ours": set(), "theirs": set()}  # ids of records only one side has
    for coll in colls:
        oi, ti, bi = index(ours, coll), index(theirs, coll), index(base, coll)
        result = []
        for rid in list(oi) + [i for i in ti if i not in oi]:
            o, t, b = oi.get(rid), ti.get(rid), bi.get(rid)
            if o is not None and t is not None:
                # _m > 0: a known edit. 0: untouched since that machine's
                # service started. Missing: a machine without the service.
                mo, mt = o.get("_m"), t.get("_m")
                if content(o) == content(t):
                    pick = o if (mo or 0) >= (mt or 0) else t
                elif mo and mt:
                    pick = o if mo >= mt else t
                elif mo and mt == 0:
                    pick = o
                elif mt and mo == 0:
                    pick = t
                elif b is not None and not mt and content(t) == content(b):
                    pick = o
                elif b is not None and not mo and content(o) == content(b):
                    pick = t
                else:
                    pick = o if (o.get("_m") or ours_mtime) >= (t.get("_m") or theirs_mtime) else t
                result.append(pick)
                continue
            r, when = (o, ours_mtime) if o is not None else (t, theirs_mtime)
            if b is not None and not r.get("_m") and content(r) == content(b):
                continue  # the other side deleted it and this side never touched it
            tomb = dels.get(f"{coll}/{rid}/{r.get('_c', birth(coll, r))}")
            if tomb is not None and tomb >= r.get("_m", when):
                continue  # deleted on the other side after its last edit here
            born["theirs" if o is None else "ours"].add(rid)
            result.append(r)
        out[coll] = result

    s = sync_meta(out)
    s["del"] = dels
    for k in set(ours) | set(theirs):
        if k == "_sync" or k in colls:
            continue
        ov, tv = ours.get(k), theirs.get(k)
        if k in COUNTERS and isinstance(ov, int) and isinstance(tv, int):
            out[k] = max(ov, tv)
            continue
        tm_o, tm_t = so["m"].get(k, ours_mtime), st["m"].get(k, theirs_mtime)
        if k in base and k in ours and k in theirs and ov != tv:
            bv = base[k]
            if isinstance(bv, list) and isinstance(ov, list) and isinstance(tv, list):
                # ours, minus what they removed, plus what they added
                out[k] = [x for x in ov if not (x in bv and x not in tv)] + \
                         [x for x in tv if x not in bv and x not in ov]
                s["m"][k] = max(tm_o, tm_t)
                continue
            if tv == bv or ov == bv:
                out[k] = ov if tv == bv else tv
                s["m"][k] = max(tm_o, tm_t)
                continue
        if k not in theirs or (k in ours and tm_o >= tm_t):
            win, lose, lose_side = ov, tv, "theirs"
        else:
            win, lose, lose_side = tv, ov, "ours"
        if isinstance(win, list) and isinstance(lose, list):
            # e.g. the dashboard: records the losing side created still belong on it
            win = win + [x for x in lose if x in born[lose_side] and x not in win]
        out[k] = win
        s["m"][k] = max(tm_o, tm_t)
    ids = [r["id"] for c in colls for r in out.get(c) or [] if isinstance(r.get("id"), int)]
    if ids:
        out["next_id"] = max(out.get("next_id", 0), max(ids) + 1)
    return out


def strip_stamps(d):
    return {k: ([content(r) if isinstance(r, dict) else r for r in v] if isinstance(v, list) else v)
            for k, v in d.items() if k != "_sync"}


def record_set(d):
    return {(c, json.dumps(content(r), sort_keys=True)) for c in collections(d) for r in d[c]}


# ---------- files ----------

def read_json(path, tries=10):
    for _ in range(tries):
        try:
            with open(path) as f:
                return json.load(f), os.stat(path)
        except FileNotFoundError:
            return None, None
        except (json.JSONDecodeError, UnicodeDecodeError):
            time.sleep(0.2)  # caught mid-write
    raise ValueError(f"{path} does not parse")


def write_json(path, d, expect=None):
    """Atomic replace; with `expect`, only if nobody wrote the file since we read it."""
    tmp = os.path.join(os.path.dirname(path), f".{os.path.basename(path)}.sync-{os.getpid()}")
    with open(tmp, "w") as f:
        json.dump(d, f, indent=2, ensure_ascii=False)
    if expect is not None:
        st = os.stat(path)
        if (st.st_mtime_ns, st.st_size, st.st_ino) != (expect.st_mtime_ns, expect.st_size, expect.st_ino):
            os.unlink(tmp)
            return False
    os.replace(tmp, path)
    return True


class Syncer:
    def __init__(self, data_dir=DATA_DIR, state_dir=STATE_DIR):
        self.data = os.path.join(data_dir, "data.json")
        self.data_dir, self.state = data_dir, state_dir
        self.base_path = os.path.join(state_dir, "base.json")
        os.makedirs(os.path.join(state_dir, "merged"), exist_ok=True)
        self.base, _ = read_json(self.base_path) if os.path.exists(self.base_path) else (None, None)
        self.seen = None
        # Conflict copies from before this service ran carry no tombstones:
        # merging them would bring back tasks deleted since. Leave those alone.
        since = os.path.join(state_dir, "since")
        if not os.path.exists(since):
            with open(since, "w") as f:
                f.write(time.strftime("%Y%m%d-%H%M%S"))
        with open(since) as f:
            self.since = f.read().strip()

    def save_base(self, d):
        changed = self.base is None or strip_stamps(self.base) != strip_stamps(d)
        self.base = d
        write_json(self.base_path, d)
        if changed:  # history: the common ancestors future merges look for
            hist = os.path.join(self.state, "history")
            os.makedirs(hist, exist_ok=True)
            write_json(os.path.join(hist, f"{now_ms()}.json"), d)
            old = sorted(glob.glob(os.path.join(hist, "*.json")))
            cutoff = now_ms() - HISTORY_MS
            for f in old[:-HISTORY_MIN]:
                if int(os.path.basename(f)[:-5]) < cutoff:
                    os.unlink(f)

    def find_base(self, theirs, ours=None):
        """The past version of this file that `theirs` most likely grew from:
        the one sharing the most identical records with it. Never a copy of
        either side itself: that is not an ancestor, and taking it as one
        makes the merge think that side changed nothing."""
        want = record_set(theirs)
        sides = [strip_stamps(theirs)] + ([strip_stamps(ours)] if ours else [])
        best, score = None, 0
        paths = glob.glob(os.path.join(self.state, "history", "*.json")) + \
            glob.glob(os.path.join(self.data_dir, "data.json.bak*")) + \
            glob.glob(os.path.join(self.data_dir, ".stversions", "data~*.json"))
        for path in paths:
            try:
                cand, _ = read_json(path, tries=1)
            except (ValueError, OSError):
                continue
            if not isinstance(cand, dict) or strip_stamps(cand) in sides:
                continue
            n = len(want & record_set(cand))
            if n > score:
                best, score = cand, n
        return best if want and score >= len(want) // 2 else None

    def stamp_pass(self):
        cur, st = read_json(self.data)
        if cur is None:
            return
        key = (st.st_mtime_ns, st.st_size, st.st_ino)
        if key == self.seen:
            return
        # First run: nothing to diff against, so every record's age is unknown
        # (0). A real edit beats it; two unknowns go by the common ancestor.
        new = stamp(cur, self.base, now=None if self.base else 0)
        if new != cur and not write_json(self.data, new, expect=st):
            return  # someone wrote meanwhile; next pass takes it
        self.seen = (lambda s: (s.st_mtime_ns, s.st_size, s.st_ino))(os.stat(self.data))
        self.save_base(new)

    def merge_pass(self):
        for path in sorted(glob.glob(os.path.join(self.data_dir, "data.sync-conflict-*.json"))):
            if os.path.basename(path)[len("data.sync-conflict-"):][:15] < self.since:
                continue
            try:
                theirs, tst = read_json(path)
            except ValueError:
                continue
            self.seen = None
            self.stamp_pass()
            ours, ost = read_json(self.data)
            base = self.find_base(theirs, ours)
            merged = merge(ours, theirs, ost.st_mtime_ns // 10**6, tst.st_mtime_ns // 10**6, base)
            merged = stamp(merged, ours)
            if not write_json(self.data, merged, expect=ost):
                return
            shutil.move(path, os.path.join(self.state, "merged", os.path.basename(path)))
            log(f"merged {os.path.basename(path)}")
            self.seen = None
            self.stamp_pass()

    def run(self, once=False):
        log(f"watching {self.data}")
        while True:
            try:
                self.merge_pass()
                self.stamp_pass()
            except Exception as e:  # keep watching; a bad pass must not stop syncing
                log(f"pass failed: {e!r}")
            if once:
                return
            time.sleep(1)


# ---------- self-tests ----------

def selftest():
    import tempfile
    import unittest

    def board(tasks, dash=None, **kw):
        d = {"version": 7, "tasks": tasks, "projects": [], "sections": [], "log": [],
             "dashboard": dash if dash is not None else [t["id"] for t in tasks],
             "daily_goal": 2, "next_id": 1 + max([t["id"] for t in tasks] or [0]), "next_color": 0}
        d.update(kw)
        return d

    def T(i, text, **kw):
        return dict(id=i, text=text, completed=False, **kw)

    def texts(d):
        return sorted((t["id"], t["text"], t.get("completed")) for t in d["tasks"])

    class Tests(unittest.TestCase):
        def setUp(self):
            self.base = stamp(board([T(1, "a"), T(2, "b"), T(3, "c")]), None, now=1000)

        def edit(self, d, now, f):
            e = copy.deepcopy(d)
            f(e)
            return stamp(e, d, now=now)

        def test_fields_an_old_build_drops_come_back(self):
            b = stamp(board([T(1, "a", note="keep")], calendar_class_edits=[{"id": "x"}]), None, now=1)
            old = json.loads(json.dumps(b))
            del old["tasks"][0]["note"], old["calendar_class_edits"]
            old["tasks"][0]["completed"] = True
            e = stamp(old, b, now=2)
            self.assertEqual(e["tasks"][0]["note"], "keep")
            self.assertTrue(e["tasks"][0]["completed"])
            self.assertEqual([content(r) for r in e["calendar_class_edits"]], [{"id": "x"}])
            self.assertEqual(e["_sync"]["del"], {}, "a dropped list is not a deletion")

        def test_restamps_only_changed_records(self):
            e = self.edit(self.base, 2000, lambda d: d["tasks"][0].update(completed=True))
            self.assertEqual([t["_m"] for t in e["tasks"]], [2000, 1000, 1000])

        def test_writer_that_drops_stamps_loses_nothing(self):
            bare = json.loads(json.dumps(self.base))
            for t in bare["tasks"]:
                t.pop("_m"); t.pop("_c")
            bare.pop("_sync")
            self.assertEqual(stamp(bare, self.base, now=9999), self.base)

        def test_edits_on_both_sides_both_survive(self):
            a = self.edit(self.base, 2000, lambda d: d["tasks"][0].update(completed=True))
            b = self.edit(self.base, 3000, lambda d: d["tasks"][1].update(text="B"))
            m = merge(a, b)
            self.assertEqual(texts(m), [(1, "a", True), (2, "B", False), (3, "c", False)])

        def test_same_record_newer_edit_wins(self):
            a = self.edit(self.base, 2000, lambda d: d["tasks"][0].update(text="old"))
            b = self.edit(self.base, 3000, lambda d: d["tasks"][0].update(text="new"))
            self.assertEqual(merge(a, b)["tasks"][0]["text"], "new")
            self.assertEqual(merge(b, a)["tasks"][0]["text"], "new")

        def test_delete_sticks_unless_edited_later(self):
            a = self.edit(self.base, 2000, lambda d: d["tasks"].pop(2))
            self.assertEqual(texts(merge(a, self.base)), [(1, "a", False), (2, "b", False)])
            self.assertEqual(texts(merge(self.base, a)), [(1, "a", False), (2, "b", False)])
            b = self.edit(self.base, 3000, lambda d: d["tasks"][2].update(text="C"))
            self.assertIn((3, "C", False), texts(merge(a, b)))

        def test_same_id_created_on_both_sides_keeps_both(self):
            def add(text):
                def f(d):
                    d["tasks"].append(T(4, text)); d["dashboard"].append(4); d["next_id"] = 5
                return f
            a = self.edit(self.base, 2000, add("from laptop"))
            b = self.edit(self.base, 2500, add("from phone"))
            m = merge(a, b)
            got = {t["text"]: t["id"] for t in m["tasks"]}
            self.assertEqual(got["from laptop"], 4)
            self.assertEqual(got["from phone"], 5)
            self.assertEqual(sorted(m["dashboard"]), [1, 2, 3, 4, 5])
            self.assertGreaterEqual(m["next_id"], 6)

        def test_same_new_task_stamped_on_two_machines_is_not_duplicated(self):
            f = lambda d: d["tasks"].append(T(4, "same"))
            a, b = self.edit(self.base, 2000, f), self.edit(self.base, 2700, f)
            self.assertEqual(len(merge(a, b)["tasks"]), 4)

        def test_unstamped_copy_merges_against_common_ancestor(self):
            legacy = lambda d: json.loads(json.dumps(strip_stamps(d)))
            anc = stamp(board([T(1, "a"), T(2, "b"), T(3, "c")], dash=[1, 2, 3]), None, now=0)
            # laptop (has the service): ticks 1 and drops 3 from the dashboard
            ours = self.edit(anc, 5000, lambda d: (d["tasks"][0].update(completed=True), d.update(dashboard=[1, 2])))
            # desktop (no service yet): renames 2, deletes 3, adds 4; edited "later"
            def desk(d):
                d["tasks"][1]["text"] = "B"; d["tasks"].pop(2); d["tasks"].append(T(4, "d")); d["dashboard"] = [1, 2, 4]
            theirs = legacy(anc); desk(theirs)
            m = merge(ours, theirs, ours_mtime=5000, theirs_mtime=9000, base=legacy(anc))
            self.assertEqual(texts(m), [(1, "a", True), (2, "B", False), (4, "d", False)])
            self.assertEqual(m["dashboard"], [1, 2, 4])

        def test_real_edit_beats_untouched_record_even_with_bad_base(self):
            # 2026-11-18: the mini stamped its stale copy (0), the laptop's real
            # edit became the conflict copy, and a base equal to it reverted it.
            anc = stamp(board([T(1, "a")]), None, now=0)
            laptop = self.edit(anc, 5000, lambda d: d["tasks"][0].update(completed=True))
            m = merge(anc, laptop, 9000, 4000, base=strip_stamps(laptop))
            self.assertTrue(m["tasks"][0]["completed"])

        def test_find_base_skips_copies_of_either_side(self):
            with tempfile.TemporaryDirectory() as tmp:
                s = Syncer(os.path.join(tmp, "d"), os.path.join(tmp, "s"))
                a = board([T(1, "a"), T(2, "b")]); b = copy.deepcopy(a); b["tasks"][0]["completed"] = True
                os.makedirs(os.path.join(tmp, "d"))
                write_json(os.path.join(tmp, "d", "data.json.bak-x"), b)
                self.assertIsNone(s.find_base(b, a))

        def test_unknown_collections_and_values_merge(self):
            a = self.edit(self.base, 2000, lambda d: d.update(daily_goal=5))
            b = self.edit(self.base, 3000, lambda d: d.update(habits=[{"id": 9, "name": "run"}]))
            m = merge(a, b)
            self.assertEqual(m["daily_goal"], 5)
            self.assertEqual(m["habits"][0]["name"], "run")

        def test_conflict_file_is_folded_in(self):
            with tempfile.TemporaryDirectory() as tmp:
                dd, sd = os.path.join(tmp, "d"), os.path.join(tmp, "s")
                os.makedirs(dd)
                write_json(os.path.join(dd, "data.json"), board([T(1, "a"), T(2, "b")]))
                s = Syncer(dd, sd)
                s.run(once=True)
                cur, _ = read_json(os.path.join(dd, "data.json"))
                theirs = copy.deepcopy(cur)
                theirs["tasks"][1]["completed"] = True
                theirs["tasks"][1]["_m"] = now_ms() + 5000
                cur["tasks"][0]["text"] = "A"
                write_json(os.path.join(dd, "data.json"), cur)
                write_json(os.path.join(dd, "data.sync-conflict-29991231-120000-ABCDEFG.json"), theirs)
                s.run(once=True)
                m, _ = read_json(os.path.join(dd, "data.json"))
                self.assertEqual(texts(m), [(1, "A", False), (2, "b", True)])
                self.assertEqual(glob.glob(os.path.join(dd, "data.sync-conflict-*")), [])
                self.assertEqual(len(os.listdir(os.path.join(sd, "merged"))), 1)
                old = os.path.join(dd, "data.sync-conflict-20000101-000000-ABCDEFG.json")
                write_json(old, theirs)
                s.run(once=True)
                self.assertTrue(os.path.exists(old), "pre-install conflict copies are left alone")

    r = unittest.TextTestRunner(verbosity=1).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests))
    sys.exit(0 if r.wasSuccessful() else 1)


if __name__ == "__main__":
    if "--test" in sys.argv:
        selftest()
    Syncer().run(once="--once" in sys.argv)
