#!/usr/bin/env python3
"""Builds the Procera website into site/dist/. Standard library only.

    python3 site/build.py            # data + pages + assets
    SITE_URL=https://example.org python3 site/build.py

Pages live in site/src/pages/*.html with a small front-matter comment. `{{ name args }}`
tokens are expanded by the functions in TOKENS below; partials come from site/src/partials.
"""
import html
import json
import os
import pathlib
import re
import runpy
import shutil
import sys
import datetime

SITE = pathlib.Path(__file__).resolve().parent
ROOT = SITE.parent
SRC = SITE / "src"
DIST = SITE / "dist"
BRAND = ROOT / "assets" / "brand" / "vector"
SITE_URL = os.environ.get("SITE_URL", "https://casualoffice.github.io/Procera").rstrip("/")
GITHUB = "https://github.com/CasualOffice/Procera"
DOCS = GITHUB + "/tree/main/docs"

sys.path.insert(0, str(SRC))
import content as C  # noqa: E402

esc = html.escape
NAV = [("index", "Home"), ("features", "Features"), ("flows", "User flows"), ("compare", "Compare"),
       ("roadmap", "Roadmap"), ("security", "Security"), ("open-source", "Open source")]

ICONS = {
    "github": '<svg viewBox="0 0 24 24" aria-hidden="true" fill="currentColor"><path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.53-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.04 0 0 .97-.31 3.17 1.18a10.9 10.9 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.58.23 2.75.11 3.04.74.81 1.19 1.83 1.19 3.09 0 4.41-2.69 5.38-5.26 5.67.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z"/></svg>',
    "sun": '<svg class="sun" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
    "moon": '<svg class="moon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z"/></svg>',
    "menu": '<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h16"/></svg>',
    "check": '<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3 4 6v6c0 4.5 3.4 8 8 9 4.6-1 8-4.5 8-9V6l-8-3Z"/><path d="m8.5 12 2.5 2.5 4.5-5"/></svg>',
    "book": '<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2V5Z"/><path d="M4 19a2 2 0 0 1 2-2h13"/></svg>',
}

# ---------------------------------------------------------------- data
runpy.run_path(str(SITE / "scripts" / "build-data.py"), run_name="__main__")
T = json.loads((SITE / "data" / "tracker.json").read_text())
EPIC = {e["id"]: e for e in T["epics"]}
REL = {r["code"]: r for r in T["releases"]}
ORDER = [r["code"] for r in T["releases"]]
MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]


def month(iso):
    y, m, _ = iso.split("-")
    return f"{MONTHS[int(m) - 1]} {y}"


def long_date(iso):
    d = datetime.date.fromisoformat(iso)
    return f"{d.day} {['January','February','March','April','May','June','July','August','September','October','November','December'][d.month-1]} {d.year}"


def feature_status(f):
    """(css class, label, source text) derived from the tracker. Never typed by hand."""
    if not f["epics"]:
        return "available", "Available now", f"No separate epic; verified by browser test <code>{esc(f['verified_by'])}</code>"
    sts = [EPIC[i]["status"] for i in f["epics"]]
    ids = ", ".join(f["epics"])
    if all(s == "Done" for s in sts):
        return "available", "Available now", f"Tracker: {ids} done"
    if any(s in ("Done", "In progress", "In review") for s in sts):
        done = sum(s == "Done" for s in sts)
        return "progress", "In progress", f"Tracker: {done} of {len(sts)} epics done ({ids})"
    rel = min((EPIC[i]["release"] for i in f["epics"]), key=ORDER.index)
    r = REL[rel]
    return "planned", f"Planned · {rel}, {month(r['end'])}", f"Tracker: {ids} not started; release {rel} targets {month(r['end'])}"


def badge(cls, label):
    return f'<span class="badge {cls}">{esc(label)}</span>'


# ---------------------------------------------------------------- screenshots
def shot(name, cls="", eager=False, caption=None, crop=True):
    phone = name == "phone-home"
    W, H = (1170, 2532) if phone else (2880, 1800)
    scale = W / (924 if phone else 2000)
    box = C.CROPS.get(name) if crop else None
    alt = C.SHOTS[name]
    if box:
        x0, y0, x1, y1 = (v * scale for v in box)
        w, h = x1 - x0, y1 - y0
        style = (f' style="width:{W / w * 100:.3f}%;margin:-{y0 / w * 100:.3f}% 0 0 -{x0 / w * 100:.3f}%"')
        ratio = f"{w:.0f} / {h:.0f}"
    else:
        style, ratio = ' style="width:100%"', f"{W} / {H}"
    load = 'fetchpriority="high"' if eager else 'loading="lazy" decoding="async"'
    dark = "" if phone else (f'<source srcset="assets/screens/{name}-dark.png" '
                             f'media="(prefers-color-scheme: dark)" data-dark>')
    src = f"assets/screens/{name}.png" if phone else f"assets/screens/{name}-light.png"
    cap = f"<figcaption>{esc(caption)}</figcaption>" if caption else ""
    return (f'<figure class="shot{" phone" if phone else ""} {cls}"><div class="frame" style="aspect-ratio:{ratio}">'
            f'<picture>{dark}<img src="{src}" alt="{esc(alt)}" width="{W}" height="{H}" {load}{style}></picture>'
            f"</div>{cap}</figure>")


# ---------------------------------------------------------------- sections
def t_pillars(_):
    out = []
    for i, (pid, name, meaning, text) in enumerate(C.PILLARS, 1):
        feats = [f for f in C.FEATURES if f["pillar"] == pid]
        avail = [f["title"] for f in feats if feature_status(f)[0] == "available"][:3]
        items = "".join(f"<li>{esc(t)}</li>" for t in avail) or "<li>Planned from R2</li>"
        out.append(f'<div class="pillar"><span class="num">0{i}</span><h3>{name}</h3>'
                   f'<p class="muted">{esc(meaning)}.</p>'
                   f'<ul aria-label="Available now in {name}">{items}</ul>'
                   f'<a class="small" href="features.html#{pid}">{name} features</a></div>')
    return f'<div class="pillars">{"".join(out)}</div>'


def t_home_caps(_):
    byid = {f["id"]: f for f in C.FEATURES}
    rows = []
    for fid in C.HOME_CAPS:
        f = byid[fid]
        cls, label, _ = feature_status(f)
        lis = "".join(f"<li>{esc(b)}</li>" for b in f["bullets"][:3])
        rows.append(f'<div class="cap"><div class="cap-text"><div>{badge(cls, label)}</div><h3>{esc(f["title"])}</h3>'
                    f'<p class="muted">{esc(f["summary"])}</p><ul>{lis}</ul>'
                    f'<a href="features.html#{fid}">Details<span class="sr-only"> about {esc(f["title"])}</span></a></div>'
                    f'{shot(f["shot"])}</div>')
    return f'<div class="caps">{"".join(rows)}</div>'


def t_features(_):
    out = []
    for pid, name, meaning, text in C.PILLARS:
        items = []
        for f in [f for f in C.FEATURES if f["pillar"] == pid]:
            cls, label, src = feature_status(f)
            lis = "".join(f"<li>{esc(b)}</li>" for b in f["bullets"])
            aside = f'<p class="aside">{esc(f["aside"])}</p>' if f.get("aside") else ""
            top = f'<div class="top"><h3>{esc(f["title"])}</h3>{badge(cls, label)}</div><p>{esc(f["summary"])}</p>'
            if f.get("shot"):
                items.append(f'<article class="feature" id="{f["id"]}"><div class="feature-text">{top}<ul>{lis}</ul>{aside}'
                             f'<p class="source">{src}</p></div>{shot(f["shot"])}</article>')
            else:  # no screenshot: summary on the left, detail on the right
                items.append(f'<article class="feature no-shot" id="{f["id"]}"><div class="feature-text">{top}'
                             f'<p class="source">{src}</p></div><div class="feature-text"><ul>{lis}</ul>{aside}</div></article>')
        out.append(f'<section class="feature-group" id="{pid}" aria-labelledby="{pid}-h"><div class="wrap">'
                   f'<header><span class="eyebrow">{name}</span><h2 id="{pid}-h">{esc(meaning)}</h2>'
                   f'<p class="muted">{esc(text)}</p></header>{"".join(items)}</div></section>')
    return "".join(out)


def t_flows_toc(_):
    return '<ol class="toc">' + "".join(f'<li><a href="#{f["id"]}">{esc(f["title"])}</a></li>' for f in C.FLOWS) + "</ol>"


def t_flows(_):
    known = set(T["e2e_ids"])
    out = []
    for f in C.FLOWS:
        missing = [t for t in f["tests"] if t not in known]
        if missing:
            raise SystemExit(f"build: flow {f['id']} cites tests not found in e2e/packs: {missing}")
        steps = "".join(f"<li><div><strong>{esc(a)}</strong> <span>{esc(b)}</span></div></li>" for a, b in f["steps"])
        tests = ", ".join(f"<code>{t}</code>" for t in f["tests"])
        note = f" {esc(f['note'])}" if f.get("note") else ""
        phone = "phone-home" in f["shots"]
        figs = "".join(shot(s) for s in f["shots"])
        cls = "flow-shots with-phone" if phone and len(f["shots"]) == 2 else "flow-shots"
        out.append(f'<section class="flow" id="{f["id"]}" aria-labelledby="{f["id"]}-h"><div class="wrap">'
                   f'<div class="flow-head"><h2 id="{f["id"]}-h">{esc(f["title"])}</h2><p class="lede">{esc(f["intro"])}</p></div>'
                   f'<div class="flow-body"><div><ol class="steps">{steps}</ol>'
                   f'<p class="verified">{ICONS["check"]}<span>Verified by automated browser test{"s" if len(f["tests"]) > 1 else ""} {tests}, '
                   f'run against the real server, database and mail catcher.{note}</span></p></div>'
                   f'<div class="{cls}">{figs}</div></div></div></section>')
    return "".join(out)


US = ' class="us"'
CUR = ' aria-current="page"'


def mark(v):
    kind, _, note = v.partition(":")
    sym, cls, sr = {"y": ("✓", "yes", "Yes"), "p": ("◐", "part", "Partial"), "n": ("–", "no", "No"),
                    "plan": ("○", "plan", "Planned")}[kind]
    n = f'<span class="note">{esc(note)}</span>' if note else ""
    return f'<span class="mark {cls}"><span class="sym" aria-hidden="true">{sym}</span><span class="sr-only">{sr}</span>{n}</span>'


def t_compare(_):
    head = "".join(f'<th scope="col"{US if c == "Procera" else ""}>{esc(c)}</th>' for c in C.COMPARE_COLS)
    rows = []
    for label, vals in C.COMPARE_ROWS:
        cells = "".join(f'<td{US if i == 4 else ""}>{mark(v)}</td>' for i, v in enumerate(vals))
        rows.append(f'<tr><th scope="row">{esc(label)}</th>{cells}</tr>')
    return (f'<div class="table-wrap" role="region" aria-label="Comparison by category" tabindex="0"><table class="compare">'
            f'<caption>✓ supported · ◐ partial, add-on or manual · – not supported · ○ planned (Procera only, from the tracker). '
            f'Scroll sideways on small screens.</caption>'
            f'<thead><tr><th scope="col">Capability</th>{head}</tr></thead><tbody>{"".join(rows)}</tbody></table></div>')


def bar(r):
    t = max(r["total"], 1)
    return (f'<div class="bar" role="img" aria-label="{r["done"]} done, {r["in_progress"]} in progress, '
            f'{r["not_started"]} not started of {r["total"]}"><i class="d" style="width:{r["done"] / t * 100:.2f}%"></i>'
            f'<i class="p" style="width:{r["in_progress"] / t * 100:.2f}%"></i></div>')


STATUS_BADGE = {"Done": ("available", "Done"), "In progress": ("progress", "In progress"),
                "In review": ("progress", "In review"), "Blocked": ("planned", "Blocked"),
                "Not started": ("planned", "Not started"), "Deferred": ("planned", "Deferred")}


def t_roadmap(_):
    out = []
    for r in T["releases"]:
        active = r["in_progress"] > 0
        now = '<span class="now">Active</span>' if active else ""
        cur = active or r["code"] == "R1"
        window = month(r["start"]) if month(r["start"]) == month(r["end"]) else f'{month(r["start"])} – {month(r["end"])}'
        epics = "".join(
            f'<li><code>{e}</code><span>{esc(EPIC[e]["title"])}</span>{badge(*STATUS_BADGE[EPIC[e]["status"]])}</li>'
            for e in r["epics"])
        out.append(
            f'<article class="release" id="{r["code"].lower()}"><div class="when"><span class="code">{r["code"]}{now}</span>'
            f'<span class="dates">{window}</span><span class="dates">Gate {esc(r["gate"])}</span></div>'
            f'<div><h3>{esc(r["name"])}</h3><p class="scope">{esc(re.sub(r" ?[(]old M[^)]*[)]", "", r["scope"]))}</p>{bar(r)}'
            f'<p class="counts"><span><i class="d"></i>{r["done"]} done</span><span><i class="p"></i>{r["in_progress"]} in progress</span>'
            f'<span><i class="n"></i>{r["not_started"]} not started</span><span>{r["total"]} epics</span></p>'
            f'<details{" open" if cur else ""}><summary>Epics in {r["code"]}</summary><ul class="epics">{epics}</ul></details></div></article>')
    return f'<div class="releases">{"".join(out)}</div>'


def t_stats(_):
    tt = T["totals"]
    r1 = REL["R1"]
    return (f'<div class="stats"><div><b>{tt["Done"]}</b><span>of {tt["all"]} epics done</span></div>'
            f'<div><b>{tt["In progress"] + tt["In review"]}</b><span>in progress</span></div>'
            f'<div><b>{r1["done"]}/{r1["total"]}</b><span>R1 epics done</span></div>'
            f'<div><b>{T["e2e_scenarios"]}</b><span>automated E2E scenarios</span></div></div>')


def simple(value):
    return lambda _: value


R1 = REL["R1"]
TOKENS = {
    "pillars": t_pillars, "home_caps": t_home_caps, "features": t_features, "flows": t_flows,
    "flows_toc": t_flows_toc, "compare": t_compare, "roadmap": t_roadmap, "stats": t_stats,
    "shot": lambda a: shot(a[0], cls=" ".join(x for x in a[1:] if x not in ("eager", "full")),
                           eager="eager" in a, crop="full" not in a),
    "icon": lambda a: ICONS[a[0]],
    "updated": simple(long_date(T["last_updated"])), "updated_iso": simple(T["last_updated"]),
    "e2e_count": simple(str(T["e2e_scenarios"])),
    "done": simple(str(T["totals"]["Done"])), "total": simple(str(T["totals"]["all"])),
    "r1_done": simple(str(R1["done"])), "r1_total": simple(str(R1["total"])), "r1_end": simple(month(R1["end"])),
    "current": simple(T["current_release"]), "ga_end": simple(month(REL["GA"]["end"])),
    "gh": simple(GITHUB), "docs": simple(DOCS),
    "legend": simple('<p class="legend"><span>Status comes from the delivery tracker:</span>'
                     + badge("available", "Available now") + badge("progress", "In progress")
                     + badge("planned", "Planned · R2 to GA") + "</p>"),
}


def expand(text, ctx):
    def rep(m):
        name, *args = m.group(1).split()
        if name in ctx:
            return ctx[name]
        if name not in TOKENS:
            raise SystemExit(f"build: unknown token {{{{ {m.group(1)} }}}}")
        return TOKENS[name](args)
    prev = None
    while prev != text:  # partials may contain tokens
        prev, text = text, re.sub(r"\{\{\s*([^}]+?)\s*\}\}", rep, text)
    return text


def front_matter(text):
    m = re.match(r"\s*<!--(.*?)-->", text, re.S)
    meta = {}
    for line in m.group(1).strip().splitlines():
        k, _, v = line.partition(":")
        meta[k.strip()] = v.strip()
    return meta, text[m.end():]


def nav_html(active, cls):
    return "".join(f'<a href="{p}.html"{CUR if p == active else ""}>{n}</a>' for p, n in NAV)


# ---------------------------------------------------------------- build
def build():
    if DIST.exists():
        shutil.rmtree(DIST)
    (DIST / "assets" / "brand").mkdir(parents=True)
    for sub in ("css", "js", "fonts", "screens"):
        shutil.copytree(SITE / "assets" / sub, DIST / "assets" / sub)
    for f in ("og.png",):
        if (SITE / "assets" / f).exists():
            shutil.copy2(SITE / "assets" / f, DIST / "assets" / f)
    for f in ("procera-logo-horizontal.svg", "procera-logo-horizontal-dark.svg", "procera-symbol.svg"):
        shutil.copy2(BRAND / f, DIST / "assets" / "brand" / f)
    shutil.copy2(BRAND / "procera-symbol-small.svg", DIST / "favicon.svg")

    partial = {p.stem: p.read_text() for p in (SRC / "partials").glob("*.html")}
    pages = sorted((SRC / "pages").glob("*.html"))
    built = []
    for p in pages:
        meta, body = front_matter(p.read_text())
        slug = p.stem
        path = "" if slug == "index" else f"{slug}.html"
        ctx = {
            "title": esc(meta["title"]), "description": esc(meta["description"]),
            "canonical": f"{SITE_URL}/{path}", "og_image": f"{SITE_URL}/assets/og.png",
            "nav": nav_html(meta.get("nav", slug), "nav"), "site_url": SITE_URL,
            "robots": meta.get("robots", "index, follow"),
        }
        head = expand(partial["head"], ctx)
        page = (f'<!doctype html>\n<html lang="en">\n{head}\n<body>\n{expand(partial["header"], ctx)}\n'
                f'<main id="main">\n{expand(body, ctx)}\n</main>\n{expand(partial["footer"], ctx)}\n</body>\n</html>\n')
        (DIST / p.name).write_text(page)
        if slug != "404":
            built.append(path)
    today = datetime.date.today().isoformat()
    urls = "".join(f"<url><loc>{SITE_URL}/{u}</loc><lastmod>{today}</lastmod></url>" for u in built)
    (DIST / "sitemap.xml").write_text('<?xml version="1.0" encoding="UTF-8"?>\n'
                                      f'<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{urls}</urlset>\n')
    (DIST / "robots.txt").write_text(f"User-agent: *\nAllow: /\n\nSitemap: {SITE_URL}/sitemap.xml\n")
    (DIST / ".nojekyll").write_text("")
    print(f"build: {len(pages)} pages -> {DIST.relative_to(ROOT)} (SITE_URL={SITE_URL})")


if __name__ == "__main__":
    build()
