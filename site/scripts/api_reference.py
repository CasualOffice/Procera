"""The API reference, generated from the router (so it cannot drift from the code).

Reads every `.route("…", get(x).post(y))` line under apps/procera/src/http, the handler's `///`
doc comment (its summary), the module's `//!` doc (the area introduction) and the audit contract
(which events a route writes). Writes site/data/api.json and an OpenAPI 3.1 document (paths,
methods, summaries, tags, bearer auth; request and response bodies are described in words).
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HTTP = ROOT / "apps" / "procera" / "src" / "http"
METHODS = ("get", "post", "put", "patch", "delete")
SKIP = {"test_controls.rs", "audit_contract.rs", "error.rs", "probe.rs"}
SYSTEM = {"system.rs", "system_console.rs", "system_email.rs"}
# area key (router file) → (name, public introduction), in reference order
AREAS = {
    "auth": ("Sign-in and account", "Signing in with a password and a second step, refreshing and ending sessions, two-step verification and passkeys. Browser flows; automation uses API tokens."),
    "email_actions": ("Email approvals", "The confirm page behind Approve and Reject links in approval emails: read the request, then decide once. The link is the credential; no session."),
    "webhooks": ("Outgoing webhooks", "Endpoints that receive signed events, their delivery log, replay, test events and secret rolling, and the event catalogue."),
    "tokens": ("API tokens", "Personal API tokens: create, list and revoke your own; security admins list and revoke everyone's."),
    "workspace": ("Workspace and members", "Your workspaces and permissions, switching workspace and step-up, invitations, members, deactivation and reactivation."),
    "directory": ("Directory", "Departments, groups, positions, managers and the CSV import that keeps them in step with your HR system."),
    "library": ("Library (SOPs and policies)", "Controlled documents: drafts, review and approval, publishing as major or minor versions, retirement, review cadence, verification and change requests."),
    "campaigns": ("Acknowledgement campaigns", "Ask people to acknowledge a published document by a deadline, with reminders, escalation to managers and evidence exports."),
    "announcements": ("Announcements", "Official messages with audiences, approval before publishing, scheduling, read receipts and required acknowledgements."),
    "requests": ("Requests", "Request types you may start, starting a request, your and all requests, request details with steps, timeline and why each step went to whom, comments and watching."),
    "work": ("My work", "Your approvals, tasks and acknowledgements: decide, reassign and delegate work items, and team workload for managers."),
    "processes": ("Processes (Guided Studio)", "Design processes: templates, drafts saved with If-Match, validation in plain language, test runs, who a step will go to, and publishing."),
    "delegation": ("Out of office and delegation", "Set and clear out of office, for yourself or (as an admin) someone else, with a delegate who decides on your behalf."),
    "files": ("Files", "Resumable uploads, virus scanning, previews and short-lived download links."),
    "search": ("Search", "Search across documents, requests, people and more, limited to what you may see."),
    "notifications": ("Notifications", "Your in-app notifications and how you are notified."),
    "profile": ("Profile", "Your name, time zone and language."),
    "views": ("Saved views and exports", "Saved table views (personal, shared, by role) and recorded CSV and XLSX exports."),
    "settings": ("Settings", "Workspace settings with layered defaults, department overrides, floors and configuration profiles."),
    "audit": ("Audit trail", "The hash-chained audit trail: search events, object timelines, chain verification, saved queries and exports."),
    "retention": ("Retention", "How long records, drafts and logs are kept, and what happens when they expire."),
    "encryption": ("Encryption keys", "The workspace's encryption keys and their rotation."),
    "overview": ("Admin overview", "What needs an administrator's attention."),
    "templates": ("Templates", "The template gallery."),
    "recovery": ("Password recovery", "Forgotten passwords, reset links, changing your password and admin account controls."),
    "system": ("System plane: workspaces", "For system administrators: create, rename, suspend, restore and schedule deletion of workspaces, and quotas. Never reads workspace content."),
    "system_console": ("System plane: console", "For system administrators: platform health and configuration."),
    "system_email": ("System plane: email", "For system administrators: the outgoing email (SMTP) settings."),
}
ROUTE = re.compile(r'^\s*\.route\("([^"]+)",\s*(.+)\)\s*$')
CALL = re.compile(r'(?:^|[.\s(])(get|post|put|patch|delete)\((\w+)\)')
EMITS = re.compile(r'emits\(\s*"(\w+)",\s*"([^"]+)",\s*&\[(.*?)\]\s*\)', re.S)
EXEMPT = re.compile(r'exempt\(\s*"(\w+)",\s*"([^"]+)",\s*"([^"]+)"', re.S)


def module_doc(src: str) -> str:
    lines = []
    for line in src.splitlines():
        if line.startswith("//!"):
            lines.append(line[3:].strip())
        elif lines:
            break
    return " ".join(lines).strip()


def handler_docs(src: str) -> dict[str, str]:
    """`async fn name` → the `///` lines just above it (attributes in between are fine)."""
    out, pending = {}, []
    for line in src.splitlines():
        s = line.strip()
        if s.startswith("///"):
            pending.append(s[3:].strip())
            continue
        m = re.match(r"(?:pub(?:\(crate\))?\s+)?async fn (\w+)", s)
        if m:
            out[m.group(1)] = " ".join(pending).strip()
            pending = []
        elif not s.startswith("#[") and s:
            pending = []
    return out


INTERNAL = re.compile(r"\s*\((?:[^()]*?\b(?:BPMS|FR|FX|CFG|OQ|A|U|SEC|ADR)-[A-Z0-9]+|[^()]*?docs/\d+)[^()]*\)")


def public(text: str) -> str:
    """Drops internal tracker and spec references (`(BPMS-047, docs/08 §3)`) from public text."""
    text = INTERNAL.sub("", text)
    text = re.sub(r"\b(?:BPMS|FR|FX|CFG|OQ)-[A-Z0-9]+(?:[/–-][A-Z0-9]+)*\b[,;:]?\s*", "", text)
    return re.sub(r"\s{2,}", " ", text).strip()


def summary_of(doc: str, method: str, path: str) -> str:
    """Drops a leading "`METHOD /path`:" (the reference shows it) and internal references."""
    d = re.sub(r"^`[A-Z]+ [^`]+`(?: and `[A-Z]+ [^`]+`)*\s*[:—-]\s*", "", doc)
    d = public(d)
    return d[:1].upper() + d[1:]


def contract() -> dict[tuple[str, str], dict]:
    src = (HTTP / "audit_contract.rs").read_text()
    body = src.split("pub const CONTRACT", 1)[1].split("#[cfg(test)]", 1)[0]
    out = {}
    for m in EMITS.finditer(body):
        alts = re.findall(r"&\[([^\[\]]*)\]", m.group(3))
        events = sorted({e for a in alts for e in re.findall(r'"([^"]+)"', a)})
        out[(m.group(1), m.group(2))] = {"audited": events}
    for m in EXEMPT.finditer(body):
        out[(m.group(1), m.group(2))] = {"exempt": m.group(3)}
    return out


def collect() -> dict:
    audit = contract()
    areas = []
    for f in sorted(HTTP.glob("*.rs")):
        if f.name in SKIP:
            continue
        src = f.read_text()
        docs = handler_docs(src)
        prefix = "/sysapi/v1" if f.name in SYSTEM else "/api/v1"
        routes = []
        for line in src.splitlines():
            m = ROUTE.match(line)
            if not m:
                continue
            path, handlers = prefix + m.group(1), m.group(2)
            for method, fn in CALL.findall(handlers):
                if method not in METHODS:
                    continue
                M = method.upper()
                doc = docs.get(fn, "")
                r = {"method": M, "path": path, "handler": fn, "summary": summary_of(doc, M, path)}
                r.update(audit.get((M, path), {}))
                routes.append(r)
        if routes:
            name, intro = AREAS.get(f.stem, (f.stem.replace("_", " ").capitalize(), public(module_doc(src))))
            areas.append({"key": f.stem, "name": name, "intro": intro, "routes": routes})
    order = list(AREAS)
    areas.sort(key=lambda a: order.index(a["key"]) if a["key"] in order else len(order))
    return {"areas": areas}


def openapi(ref: dict, version: str) -> dict:
    paths: dict = {}
    for a in ref["areas"]:
        for r in a["routes"]:
            op = {"tags": [a["name"]], "operationId": f'{a["key"]}_{r["handler"]}',
                  "summary": (r["summary"].split(". ")[0] or f'{r["method"]} {r["path"]}')[:200],
                  "description": r["summary"],
                  "responses": {"default": {"description": "See the description; errors are RFC 9457 problem details.",
                                            "content": {"application/problem+json": {}}}}}
            if r.get("audited"):
                op["x-audit-events"] = r["audited"]
            params = re.findall(r"\{(\w+)\}", r["path"])
            if params:
                op["parameters"] = [{"name": p, "in": "path", "required": True, "schema": {"type": "string"}} for p in params]
            paths.setdefault(r["path"], {})[r["method"].lower()] = op
    return {
        "openapi": "3.1.0",
        "info": {"title": "Procera API", "version": version,
                 "description": "Generated from the server's router. Request and response bodies are described in words; typed schemas are planned."},
        "servers": [{"url": "https://your-procera.example.com"}],
        "components": {"securitySchemes": {"bearer": {"type": "http", "scheme": "bearer",
                                                      "description": "A session access token or a personal API token (prc_live_…)."}}},
        "security": [{"bearer": []}],
        "paths": paths,
    }


if __name__ == "__main__":
    ref = collect()
    n = sum(len(a["routes"]) for a in ref["areas"])
    missing = [f'{r["method"]} {r["path"]}' for a in ref["areas"] for r in a["routes"] if not r["summary"]]
    print(f"{len(ref['areas'])} areas, {n} operations, {len(missing)} without a summary")
    for m in missing[:400]:
        print("  no summary:", m)
    if "--check" in sys.argv:
        sys.exit(1 if missing else 0)
    if len(sys.argv) > 1:
        out = Path(sys.argv[1])
        out.mkdir(parents=True, exist_ok=True)
        (out / "api.json").write_text(json.dumps(ref, indent=1, ensure_ascii=False))
        (out / "openapi.json").write_text(json.dumps(openapi(ref, "0.4.0-dev"), indent=1, ensure_ascii=False))
