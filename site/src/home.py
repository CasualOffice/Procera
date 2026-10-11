"""Home-page sections (v2): proof numbers, the core loop, the product tour, personas, where
Procera leads, the hash-chain demo and the release timeline. Everything that states a status
goes through build.feature_status, so it comes from the tracker."""
from html import escape as esc

import content as C
import illustrations as IL

TOUR_URL = {"library": "/library/acceptable-use-of-it", "campaigns": "/admin/campaigns/office-closed",
            "announcements": "/announcements", "audit": "/admin/audit", "roles": "/admin/roles"}
LOOP_ICONS = [  # 24-unit line icons: pen, two people, globe-ish send, check, file, chain
    '<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>',
    '<circle cx="9" cy="8" r="3"/><path d="M3 20a6 6 0 0 1 12 0"/><path d="m15 11 2 2 4-4"/>',
    '<path d="M12 3v12"/><path d="m7 8 5-5 5 5"/><path d="M5 15v4a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4"/>',
    '<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>',
    '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/><path d="M9 13h6M9 17h4"/>',
    '<rect x="2" y="8" width="9" height="8" rx="4"/><rect x="13" y="8" width="9" height="8" rx="4"/><path d="M9 12h6"/>',
]


def tokens(B):
    byid = {f["id"]: f for f in C.FEATURES}

    def status(ref):
        return B.feature_status(byid[ref] if isinstance(ref, str) else {"epics": ref})

    def mini(ref):
        cls, label, _ = status(ref)
        return B.badge(cls, label)

    def illus(a):
        return IL.SCENES[a[0]]()

    def proof(_):
        tt, r1 = B.T["totals"], B.REL["R1"]
        items = [
            (f'<span data-count="{tt["Done"]}">{tt["Done"]}</span>', f'epics done, of {tt["all"]} on the public tracker'),
            (f'<span data-count="{B.T["e2e_scenarios"]}">{B.T["e2e_scenarios"]}</span>', "automated end-to-end scenarios against a real server and database"),
            (f'<span data-count="{r1["done"]}">{r1["done"]}</span><small>/{r1["total"]}</small>', f'epics done for R1, the first usable release ({B.month(r1["end"])})'),
            ('<span data-count="0">0</span>', "telemetry by default. Your data stays on your servers"),
        ]
        cells = "".join(f'<div class="stat rv" style="--i:{i}"><b>{n}</b><span>{esc(t)}</span></div>' for i, (n, t) in enumerate(items))
        return f'<div class="proof">{cells}</div>'

    def loop(_):
        wave_h = "M100 40C160 10 240 10 300 40S440 70 500 40S640 10 700 40S840 70 900 40S1040 10 1100 40"
        wave_v = "M30 100C10 160 10 240 30 300S50 440 30 500S10 640 30 700S50 840 30 900S10 1040 30 1100"
        steps = "".join(
            f'<li class="rv" style="--i:{i}"><span class="node"><svg viewBox="0 0 24 24" aria-hidden="true">{LOOP_ICONS[i]}</svg></span>'
            f'<div><b><span class="loop-n">0{i + 1}</span>{esc(t)}</b><p>{esc(d)}</p></div></li>'
            for i, (t, d) in enumerate(C.LOOP))
        return ('<div class="loop draw">'
                f'<svg class="loop-line h" viewBox="0 0 1200 80" preserveAspectRatio="none" aria-hidden="true">'
                f'<path class="loop-path" pathLength="1" d="{wave_h}"/>'
                '<path class="loop-back" d="M300 34C270 -26 130 -26 100 34"/></svg>'
                f'<svg class="loop-line v" viewBox="0 0 60 1200" preserveAspectRatio="none" aria-hidden="true">'
                f'<path class="loop-path" pathLength="1" d="{wave_v}"/></svg>'
                '<span class="loop-note" aria-hidden="true">changes requested</span>'
                f'<ol class="loop-steps">{steps}</ol></div>')

    def tour(_):
        steps, layers = [], []
        for i, fid in enumerate(C.HOME_CAPS):
            f = byid[fid]
            lis = "".join(f"<li>{esc(b)}</li>" for b in f["bullets"][:3])
            steps.append(
                f'<article class="tour-step" data-i="{i}" data-url="{TOUR_URL[fid]}"><span class="tour-num">0{i + 1}</span>'
                f'<div class="tour-meta">{mini(fid)}</div><h3>{esc(f["title"])}</h3><p class="muted">{esc(f["summary"])}</p>'
                f'<ul>{lis}</ul><a class="more" href="features.html#{fid}">How it works<span class="sr-only">: {esc(f["title"])}</span></a>'
                f'<div class="tour-inline">{B.shot(f["shot"])}</div></article>')
            load = 'loading="lazy" decoding="async"' if i else 'decoding="async"'
            # Zoom on the screen's content column (its crop box), cut to the frame's 16:10.
            x0, y0, x1, _ = C.CROPS.get(f["shot"], (0, 0, 2000, 1250))
            w = x1 - x0
            style = f' style="width:{2000 / w * 100:.2f}%;max-width:none;margin:-{y0 / w * 100:.2f}% 0 0 -{x0 / w * 100:.2f}%"'
            layers.append(f'<div class="layer{" on" if i == 0 else ""}" data-i="{i}">'
                          f'{B.picture(f["shot"], C.SHOTS[f["shot"]], 2880, 1800, load, style)}</div>')
        frame = ('<div class="tour-frame"><div class="browser"><div class="browser-bar"><span class="dots"><i></i><i></i><i></i></span>'
                 f'<span class="url">procera.example.com<b data-url>{TOUR_URL[C.HOME_CAPS[0]]}</b></span></div>'
                 f'<div class="browser-view">{"".join(layers)}</div></div>'
                 '<ol class="tour-dots" aria-hidden="true">' + "".join(f'<li data-i="{i}"></li>' for i in range(len(layers))) + '</ol></div>')
        return f'<div class="tour-grid"><div class="tour-steps">{"".join(steps)}</div>{frame}</div>'

    def personas(_):
        tabs, panels = [], []
        for i, p in enumerate(C.PERSONAS):
            sel = i == 0
            tabs.append(f'<button class="ptab" role="tab" id="tab-{p["id"]}" aria-controls="panel-{p["id"]}" '
                        f'aria-selected="{"true" if sel else "false"}" tabindex="{0 if sel else -1}" type="button">{esc(p["tab"])}</button>')
            pts = "".join(
                f'<li>{"<span class=ok aria-hidden=true></span>" if status(r)[0] == "available" else ""}<span>{esc(t)}</span>'
                f'{"" if status(r)[0] == "available" else mini(r)}</li>' for t, r in p["points"])
            nt, nr = p["next"]
            nxt = f'<p class="pnext"><span>Next: {esc(nt)}</span>{mini(nr)}</p>'
            shot = B.shot(p["shot"])
            panels.append(
                f'<div class="ppanel{" on" if sel else ""}" role="tabpanel" id="panel-{p["id"]}" aria-labelledby="tab-{p["id"]}" tabindex="0"{"" if sel else " hidden"}>'
                f'<div class="ppanel-copy"><span class="eyebrow">{esc(p["role"])}</span><h3>{esc(p["title"])}</h3>'
                f'<p class="muted">{esc(p["lede"])}</p><ul class="pts">{pts}</ul>{nxt}</div>'
                f'<div class="ppanel-visual{" is-phone" if p["shot"] == "phone-home" else ""}">'
                f'<div class="ppanel-art">{IL.PERSONA_ART[p["art"]]()}</div><div class="ppanel-shot">{shot}</div></div></div>')
        return (f'<div class="personas" data-tabs><div class="ptabs" role="tablist" aria-label="See Procera as">{"".join(tabs)}</div>'
                f'{"".join(panels)}</div>')

    def leads(_):
        rows = "".join(
            f'<li class="rv" style="--i:{i}"><span class="lead-n">0{i + 1}</span><div><h3>{esc(t)}</h3><p class="muted">{esc(d)}</p></div>'
            f'<div class="lead-st">{mini(r)}</div></li>' for i, (t, d, r) in enumerate(C.LEADS))
        return f'<ol class="leads">{rows}</ol>'

    def chain(_):
        recs = [("1041", "document.published", "Olivia Owner", "7c1e09ab", "52f0c3d1"),
                ("1042", "acknowledgement.recorded", "Ben Baker", "a90f44e2", "7c1e09ab"),
                ("1043", "acknowledgement.recorded", "Cara Chen", "3b2d71c8", "a90f44e2"),
                ("1044", "evidence.exported", "Olivia Owner", "e07a5f16", "3b2d71c8")]
        blocks = "".join(
            f'<li class="blk" data-n="{n}"><div class="blk-top"><span class="blk-n">#{n}</span><code class="blk-h" data-h>{h}</code></div>'
            f'<code class="blk-e">{e}</code><span class="blk-who" data-who>{w}</span>'
            f'<span class="blk-p">prev <code>{p}</code></span></li>' for n, e, w, h, p in recs)
        return ('<div class="chain" data-chain><p class="chain-cap">How verification works (an illustration, with shortened hashes)</p>'
                f'<ol class="chain-list">{blocks}</ol><div class="chain-ctl">'
                '<button class="btn btn-secondary btn-sm" type="button" data-tamper>Edit record #1042</button>'
                '<button class="btn btn-secondary btn-sm" type="button" data-restore hidden>Undo the edit</button>'
                '<p class="chain-status" role="status" data-status><span class="ok" aria-hidden="true"></span>Chain verified: 4 records</p></div></div>')

    def timeline(_):
        out = []
        for i, code in enumerate(["R1", "R2", "R3", "R4", "R5", "GA"]):
            r = B.REL[code]
            t = max(r["total"], 1)
            state = "Active" if r["in_progress"] else ("Done" if r["done"] == r["total"] else "Planned")
            name = r["name"].replace("&", "and")
            out.append(f'<li class="rv" style="--i:{i}"><span class="tl-code">{code}</span><span class="tl-when">{B.month(r["end"])}</span>'
                       f'<b>{esc(name)}</b><span class="tl-bar" role="img" aria-label="{r["done"]} of {r["total"]} epics done">'
                       f'<i style="--w:{r["done"] / t * 100:.1f}%"></i></span><span class="tl-state">{state} · {r["done"]}/{r["total"]}</span></li>')
        return f'<ol class="timeline">{"".join(out)}</ol>'

    def pillars(_):
        out = []
        for i, (pid, name, meaning, text) in enumerate(C.PILLARS, 1):
            feats = [f for f in C.FEATURES if f["pillar"] == pid]
            avail = [f["title"] for f in feats if B.feature_status(f)[0] == "available"][:3]
            items = "".join(f"<li>{esc(t)}</li>" for t in avail) or "<li>Planned from R2</li>"
            out.append(f'<article class="pillar2 p-{pid} rv" style="--i:{i - 1}"><div class="pillar2-art">{IL.PILLAR_ART[pid]()}</div>'
                       f'<div class="pillar2-copy"><span class="num">0{i} · {name}</span><h3>{esc(meaning)}</h3><p class="muted">{esc(text)}</p>'
                       f'<ul aria-label="Available now in {name}">{items}</ul>'
                       f'<a class="more" href="features.html#{pid}">{name} features</a></div></article>')
        return f'<div class="pillars2">{"".join(out)}</div>'

    return {"illus": illus, "proof": proof, "loop": loop, "tour": tour, "personas": personas,
            "leads": leads, "chain": chain, "timeline": timeline, "pillars": pillars,
            "story": lambda _: (B.SRC / "partials" / "story.html").read_text()}
