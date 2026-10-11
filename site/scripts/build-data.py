#!/usr/bin/env python3
"""Writes site/data/tracker.json from the delivery tracker.

The source of truth is scripts/build_tracker.py (MILESTONES, CORE, STATUS and the
release mapping). That script writes the tracker files when it is imported, so we
never import it: we parse it with `ast` and evaluate only its literal assignments.
The script is read-only for the site; nothing here edits it.

    python3 site/scripts/build-data.py
"""
import ast
import datetime
import json
import pathlib
import re
import subprocess
from collections import Counter

SITE = pathlib.Path(__file__).resolve().parent.parent
ROOT = SITE.parent
TRACKER = ROOT / "scripts" / "build_tracker.py"
OUT = SITE / "data" / "tracker.json"

WANTED = {"MILESTONES", "OLD_TO_REL", "REL_OVERRIDE", "CORE", "STATUS"}
STATUSES = ["Done", "In review", "In progress", "Blocked", "Not started", "Deferred"]


def literals(path):
    """First literal assignment of each wanted name (CORE is reassigned later by a comprehension)."""
    found = {}
    for node in ast.parse(path.read_text()).body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            t = node.targets[0]
            if isinstance(t, ast.Name) and t.id in WANTED and t.id not in found:
                try:
                    found[t.id] = ast.literal_eval(node.value)
                except ValueError:
                    pass
    missing = WANTED - found.keys()
    if missing:
        raise SystemExit(f"build-data: could not read {sorted(missing)} from {path}")
    return found


def last_updated():
    """Date of the last commit that touched the tracker source, else today."""
    try:
        out = subprocess.run(
            ["git", "log", "-1", "--format=%cs", "--", str(TRACKER)],
            cwd=ROOT, capture_output=True, text=True, check=True,
        ).stdout.strip()
        if out:
            return out
    except (OSError, subprocess.CalledProcessError):
        pass
    return datetime.date.today().isoformat()


def e2e_ids():
    """Scenario ids implemented as Playwright tests (same rule as the tracker dashboard)."""
    ids = set()
    for f in (ROOT / "e2e" / "packs").rglob("*.spec.ts"):
        ids |= set(re.findall(r"test\(\s*'(E2E-[A-Z0-9-]+)", f.read_text()))
    return sorted(ids)


def main():
    d = literals(TRACKER)
    status = {k: v[0] for k, v in d["STATUS"].items()}
    rel_of = lambda r: d["REL_OVERRIDE"].get(r[0], d["OLD_TO_REL"].get(r[4], r[4]))
    order = [m[0] for m in d["MILESTONES"]]

    epics = []
    for r in d["CORE"]:
        epics.append({
            "id": r[0], "area": r[1], "title": r[2], "priority": r[3],
            "release": rel_of(r), "status": status.get(r[0], "Not started"),
        })
    epics.sort(key=lambda e: (order.index(e["release"]), e["id"]))

    releases = []
    for code, name, start, end, gate, scope in d["MILESTONES"]:
        items = [e for e in epics if e["release"] == code]
        cnt = Counter(e["status"] for e in items)
        releases.append({
            "code": code, "name": name, "start": start, "end": end, "gate": gate, "scope": scope,
            "total": len(items),
            "done": cnt.get("Done", 0),
            "in_progress": cnt.get("In progress", 0) + cnt.get("In review", 0),
            "not_started": cnt.get("Not started", 0) + cnt.get("Blocked", 0) + cnt.get("Deferred", 0),
            "epics": [e["id"] for e in items],
        })
    current = next((r["code"] for r in releases if r["done"] < r["total"]), releases[-1]["code"])
    cnt = Counter(e["status"] for e in epics)
    ids = e2e_ids()
    data = {
        "source": "scripts/build_tracker.py",
        "last_updated": last_updated(),
        "current_release": current,
        "totals": {s: cnt.get(s, 0) for s in STATUSES} | {"all": len(epics)},
        "e2e_scenarios": len(ids),
        "e2e_ids": ids,
        "releases": releases,
        "epics": epics,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n")
    print(f"build-data: {len(epics)} epics, {len(releases)} releases, {len(ids)} E2E ids -> {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
