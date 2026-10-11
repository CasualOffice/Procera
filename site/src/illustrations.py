"""Original inline-SVG illustrations for the site, built from small primitives.

One stroke/fill system (styled in assets/css/illus.css, so every drawing follows the theme):
  k   ink outline, no fill          pp  paper card with ink outline     ps  second paper tone
  sh  hard offset shadow            ln  text-line placeholder           kx  solid ink
  tN / fN  pastel fill / its text colour (N = 0 blue, 1 green, 2 purple, 3 orange, 4 pink, 5 yellow;
           the product's avatar and tag pastels)
  plN pillar plate                  rb / rs  brand ribbon band and stem (flat brand reds)
  dash  dashed outline, used for anything that is planned rather than built
Motion hooks (only run under prefers-reduced-motion: no-preference): .blink, .flowdash, .pop.

The P-ribbon (BAND/STEM) is the brand symbol's own geometry from assets/brand/vector/build.py.
"""
from html import escape

BAND = 'M15 8H38A18 18 0 0 1 38 44H31V31H38A5 5 0 0 0 38 21H15A5 5 0 0 1 10 16V13A5 5 0 0 1 15 8Z'
STEM = ('M31 31V44C26.6 47.4 21.8 51.8 17.6 56.2C15.8 58.1 13.6 59.4 11.4 59.4C9.3 59.4 8 57.8 8 55.8'
        'V49.6C8 46.5 9.4 43.6 11.9 41.7L22.8 33.4C25.1 31.8 28 31 31 31Z')
CHECK = "M-3.2 0.2 -1 2.4 3.4-2.2"


def svg(body, label, w=320, h=220, cls=""):
    return (f'<svg class="il {cls}" viewBox="0 0 {w} {h}" role="img" aria-label="{escape(label)}" '
            f'focusable="false">{body}</svg>')


def card(x, y, w, h, cls="pp", r=8, shadow=True):
    s = f'<rect class="sh" x="{x + 4}" y="{y + 4}" width="{w}" height="{h}" rx="{r}"/>' if shadow else ""
    return s + f'<rect class="{cls}" x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}"/>'


def lines(x, y, widths, gap=9, h=4, cls="ln"):
    return "".join(f'<rect class="{cls}" x="{x}" y="{y + i * gap}" width="{w}" height="{h}" rx="{h / 2}"/>'
                   for i, w in enumerate(widths))


def text(x, y, s, size=9, cls="kt", anchor="start", weight=None, mono=False):
    a = f' text-anchor="{anchor}"' if anchor != "start" else ""
    wt = f' font-weight="{weight}"' if weight else ""
    m = ' class="mono ' + cls + '"' if mono else f' class="{cls}"'
    return f'<text x="{x}" y="{y}" font-size="{size}"{a}{wt}{m}>{escape(s)}</text>'


def avatar(cx, cy, r, ini, tone):
    return (f'<circle class="t{tone}" cx="{cx}" cy="{cy}" r="{r}"/><circle class="k ring" cx="{cx}" cy="{cy}" r="{r}"/>'
            + text(cx, cy + r * 0.32, ini, round(r * 0.78, 1), f"f{tone}", "middle", 600))


def check(cx, cy, r=6, tone=1, cls=""):
    k = r / 4.6
    return (f'<g class="{cls}" transform="translate({cx} {cy})"><circle class="t{tone} badge" r="{r}"/>'
            f'<path class="cf{tone}" transform="scale({k:.2f})" d="{CHECK}"/></g>')


def tag(x, y, label, tone, w=None):
    w = w or round(len(label) * 4.9 + 18)
    return (f'<rect class="t{tone}" x="{x}" y="{y}" width="{w}" height="14" rx="4"/>'
            f'<circle class="d{tone}" cx="{x + 7}" cy="{y + 7}" r="2"/>' + text(x + 12, y + 10, label, 8, f"f{tone}", weight=550))


def ribbon(x, y, s=1.0, cls=""):
    return (f'<g class="ribbon {cls}" transform="translate({x} {y}) scale({s})">'
            f'<path class="rs" d="{STEM}"/><path class="rb" d="{BAND}"/></g>')


def plate(tone, w=320, h=220):
    return f'<rect class="pl{tone}" x="6" y="6" width="{w - 12}" height="{h - 12}" rx="18"/>'


# ------------------------------------------------------------------ pillars
def design():
    b = plate(2)
    b += f'<g transform="rotate(-5 140 120)">{card(62, 40, 150, 160, "ps")}{text(76, 60, "v1", 8, "kt3", mono=True)}{lines(76, 70, [96, 70, 110, 84], 10)}</g>'
    b += card(104, 26, 168, 176)
    b += tag(118, 40, "Policy", 2) + tag(166, 40, "In review", 5)
    b += '<rect class="kx" x="118" y="64" width="108" height="8" rx="2"/>'
    b += lines(118, 82, [136, 112, 128], 10)
    b += '<rect class="t0" x="118" y="116" width="140" height="30" rx="4"/><rect class="f0bg" x="118" y="116" width="3" height="30" rx="1"/>'
    b += lines(128, 124, [104, 76], 9, cls="ln0")
    b += "".join(f'<circle class="kd" cx="{110 + (i % 2) * 4}" cy="{124 + (i // 2) * 5}" r="1.2"/>' for i in range(6))
    b += lines(118, 156, [120, 60], 10)
    b += '<rect class="caret blink" x="181" y="164" width="1.6" height="8"/>'
    b += '<circle class="pp" cx="252" cy="182" r="24"/>' + ribbon(234, 162, 0.58)
    return svg(b, "Illustration: a policy page in the block editor, marked In review, with an earlier version behind it and the Procera ribbon as its seal")


def execute():
    b = plate(3)
    b += card(22, 56, 136, 104)
    b += ('<path class="k" d="M36 76h6l10-6v20l-10-6h-6z"/><path class="k" d="M56 76a6 6 0 0 1 0 8"/>')
    b += tag(66, 72, "Confirm by Fri", 3, 80)
    b += '<rect class="kx" x="36" y="98" width="96" height="7" rx="2"/>' + lines(36, 114, [108, 90, 64], 10)
    for i, (y, tone, ini) in enumerate([(52, 0, "AK"), (112, 1, "BB"), (172, 4, "CC")]):
        b += f'<path class="k flowdash" d="M158 108C200 108 200 {y} 240 {y}" stroke-dasharray="3 4"/>'
        b += avatar(262, y, 17, ini, tone) + check(276, y + 13, 6, 1, "pop")
    b += ('<circle class="track" cx="198" cy="108" r="16"/>'
          '<circle class="prog" cx="198" cy="108" r="16" pathLength="100" stroke-dasharray="66 100" transform="rotate(-90 198 108)"/>')
    b += text(198, 111, "2/3", 8, "kt", "middle", 600)
    return svg(b, "Illustration: an announcement that needs confirmation fans out to three people, two of whom have acknowledged")


def govern():
    b = plate(1)
    for i, (x, n, h) in enumerate([(18, "#1041", "7c1e"), (122, "#1042", "a90f"), (226, "#1043", "3b2d")]):
        b += card(x, 84, 76, 64)
        b += text(x + 10, 102, n, 9, "kt", mono=True, weight=600) + lines(x + 10, 110, [52, 38], 8)
        b += f'<rect class="t1" x="{x + 10}" y="128" width="40" height="12" rx="3"/>' + text(x + 14, 137, h + "…", 7, "f1", mono=True)
        if i < 2:
            lx = x + 80
            b += (f'<rect class="k" x="{lx - 2}" y="112" width="18" height="9" rx="4.5"/>'
                  f'<rect class="k" x="{lx + 10}" y="112" width="18" height="9" rx="4.5"/>')
    b += ('<path class="sh" d="M164 22l30 10v18c0 16-12 26-30 31-18-5-30-15-30-31V32z" transform="translate(4 4)"/>'
          '<path class="pp" d="M164 22l30 10v18c0 16-12 26-30 31-18-5-30-15-30-31V32z"/>')
    b += ribbon(150, 30, 0.44)
    b += '<rect class="t1" x="104" y="166" width="112" height="22" rx="11"/>' + check(120, 177, 6, 1)
    b += text(132, 180.5, "Chain verified", 9, "f1", weight=600)
    return svg(b, "Illustration: three audit records linked by their hashes under a shield, with a Chain verified badge")


def improve():
    b = plate(0)
    b += card(26, 28, 268, 164)
    b += '<path class="k soft" d="M50 164H272M50 164V48"/>'
    for i, (h, tone) in enumerate([(70, 3), (58, 5), (46, 1), (34, 1), (24, 1)]):
        x = 70 + i * 40
        b += f'<rect class="t{tone}" x="{x}" y="{164 - h}" width="22" height="{h}" rx="3"/><rect class="k" x="{x}" y="{164 - h}" width="22" height="{h}" rx="3"/>'
    b += '<path class="rk" d="M81 84C120 88 150 120 190 128S250 138 261 136"/><circle class="rd" cx="261" cy="136" r="3.5"/>'
    b += card(196, 44, 84, 40)
    b += text(206, 60, "Overdue", 8, "kt2") + text(206, 75, "falling", 10, "kt", weight=600)
    b += '<path class="f1s" d="M262 62v10m-4-4 4 4 4-4"/>'
    b += '<circle class="pp" cx="120" cy="112" r="15"/><circle class="k" cx="120" cy="112" r="15"/><path class="k thick" d="M131 123l10 10"/>'
    return svg(b, "Illustration: a chart of overdue items falling week by week, with a magnifier over one bar")


# ------------------------------------------------------------------ personas
def hr():
    b = plate(4)
    b += card(20, 40, 140, 140)
    b += ('<path class="pp" d="M40 64h40c6 0 10 4 10 10v84c0-5-4-8-10-8H40z"/><path class="pp" d="M140 64h-40c-6 0-10 4-10 10v84c0-5 4-8 10-8h40z"/>')
    b += lines(48, 78, [32, 24, 30, 20], 9) + lines(98, 78, [32, 28, 22, 30], 9)
    b += tag(40, 160, "Handbook v3", 4, 70)
    people = [(0, "AK", 1), (1, "BB", 1), (2, "CC", 1), (3, "DP", None), (5, "ER", None)]
    for i, (tone, ini, ok) in enumerate(people):
        cx = 196 + (i % 3) * 38
        cy = 70 + (i // 3) * 54
        b += avatar(cx, cy, 15, ini, tone)
        b += check(cx + 11, cy + 11, 5.5, 1, "pop") if ok else (f'<g transform="translate({cx + 11} {cy + 11})"><circle class="t5 badge" r="5.5"/>'
                                                            f'<path class="cf5" d="M0-2.6V0l1.8 1.4"/></g>')
    b += '<rect class="t1" x="184" y="160" width="104" height="20" rx="10"/>' + text(236, 173.5, "3 of 5 acknowledged", 8, "f1", "middle", 600)
    return svg(b, "Illustration: an employee handbook beside five people, three of whom have acknowledged it")


def manager():
    b = plate(3)
    b += avatar(160, 46, 20, "MK", 3)
    b += '<path class="k" d="M160 66v18M80 84h160M80 84v16M160 84v16M240 84v16"/>'
    for x, tone, ini in [(80, 0, "AK"), (160, 1, "BB"), (240, 4, "CC")]:
        b += avatar(x, 118, 17, ini, tone)
    b += check(91, 129, 5.5, 1) + check(171, 129, 5.5, 1)
    b += ('<g class="pop"><circle class="rd" cx="252" cy="104" r="7"/>'
          '<path class="wk" d="M249.5 103.5a2.5 2.5 0 0 1 5 0v2h-5z M251.5 107h1"/></g>')
    b += card(176, 152, 120, 42)
    b += text(188, 168, "Escalation", 8, "kt2") + text(188, 184, "1 overdue on your team", 8.5, "kt", weight=600)
    b += card(24, 152, 132, 42, "pp dash", shadow=False)
    b += text(36, 168, "Team approvals", 8, "kt2") + text(36, 184, "planned with requests", 8.5, "kt2")
    return svg(b, "Illustration: a manager above three team members, with an escalation for the one who is overdue and a dashed card for team approvals, which are planned")


def employee():
    b = plate(0)
    b += card(104, 14, 112, 196, "pp", r=18)
    b += '<rect class="k" x="146" y="22" width="28" height="5" rx="2.5"/>'
    b += text(118, 48, "For you", 9, "kt", weight=650)
    b += card(116, 58, 88, 74, "ps", r=6, shadow=False)
    b += tag(124, 66, "Policy", 2) + '<rect class="kx" x="124" y="88" width="64" height="6" rx="2"/>' + lines(124, 100, [70, 52], 8)
    b += '<rect class="kx" x="124" y="114" width="72" height="12" rx="3"/>' + text(160, 122.5, "Acknowledge", 7, "wt", "middle", 600)
    b += card(116, 140, 88, 40, "ps", r=6, shadow=False) + tag(124, 148, "Announcement", 3, 70) + lines(124, 168, [60], 8)
    b += check(204, 120, 11, 1, "pop")
    b += '<path class="k soft" d="M58 70l18 10M50 112h22M58 154l18-10M262 70l-18 10M270 112h-22M262 154l-18-10"/>'
    return svg(b, "Illustration: Procera on a phone, with a policy to acknowledge and an announcement under For you")


def it_security():
    b = plate(1)
    b += card(22, 30, 166, 160)
    b += text(36, 52, "People and access", 9, "kt", weight=650)
    for i, (tone, ini, role, rt, mfa) in enumerate([(0, "AK", "Admin", 2, True), (1, "BB", "Member", 0, True), (4, "CC", "Member", 0, False)]):
        y = 74 + i * 38
        b += avatar(46, y, 11, ini, tone) + lines(64, y - 6, [44, 30], 8) + tag(118, y - 7, role, rt, 50)
        b += f'<line class="k soft" x1="36" x2="176" y1="{y + 19}" y2="{y + 19}"/>' if i < 2 else ""
    b += ('<path class="sh" d="M244 40l36 12v22c0 20-15 32-36 38-21-6-36-18-36-38V52z" transform="translate(4 4)"/>'
          '<path class="pp" d="M244 40l36 12v22c0 20-15 32-36 38-21-6-36-18-36-38V52z"/>'
          '<circle class="rd" cx="244" cy="72" r="6"/><path class="rs" d="M241.5 74h5l2 14h-9z"/>')
    b += card(206, 134, 92, 52)
    b += '<path class="k" d="M226 168v-6a8 8 0 0 1 16 0v6M222 160a12 12 0 0 1 24 0M234 168v-6"/>'
    b += text(254, 156, "Passkey", 8, "kt2") + text(254, 170, "step-up", 9, "kt", weight=600)
    return svg(b, "Illustration: a list of people with roles beside a shield with a keyhole and a passkey step-up card")


def finance():
    b = plate(5)
    b += ('<path class="sh" d="M40 28h120v164l-12-8-12 8-12-8-12 8-12-8-12 8-12-8-12 8-12-8-12 8z" transform="translate(4 4)"/>'
          '<path class="pp" d="M40 28h120v164l-12-8-12 8-12-8-12 8-12-8-12 8-12-8-12 8-12-8-12 8z"/>')
    b += text(54, 52, "Spending policy", 9, "kt", weight=650) + tag(54, 60, "Published", 1)
    for i in range(4):
        y = 92 + i * 18
        b += lines(54, y, [54], 0) + f'<rect class="ln" x="{122}" y="{y}" width="24" height="4" rx="2"/>'
    b += '<circle class="pp" cx="214" cy="78" r="34"/><circle class="k" cx="214" cy="78" r="34"/>'
    b += check(214, 78, 16, 1)
    b += text(214, 128, "Reviewed by a second person", 8, "kt2", "middle")
    b += '<rect class="k dash" x="176" y="146" width="116" height="44" rx="8"/>'
    b += text(188, 164, "Expense approvals", 8.5, "kt", weight=600) + text(188, 179, "planned with requests", 8, "kt2")
    return svg(b, "Illustration: a published spending policy with a second-person review stamp, and a dashed card for expense approvals, which are planned")


def compliance():
    b = plate(2)
    b += card(26, 34, 132, 158)
    b += '<rect class="kx" x="68" y="26" width="48" height="16" rx="4"/>'
    for i in range(4):
        y = 66 + i * 28
        b += check(46, y, 7, 1) + lines(60, y - 4, [72, 46], 8)
    b += card(186, 34, 112, 74)
    b += '<path class="k" d="M200 46h40l14 14v38h-54z M240 46v14h14"/>'
    b += text(227, 88, "CSV", 10, "kt", "middle", 700, mono=True) + text(259, 84, "Evidence", 7.5, "kt2")
    for i, h in enumerate(["7c1e", "a90f", "3b2d"]):
        x = 186 + i * 36
        b += f'<rect class="t1" x="{x}" y="138" width="32" height="20" rx="4"/>' + text(x + 16, 151, h, 7, "f1", "middle", mono=True)
        if i < 2:
            b += f'<path class="k" d="M{x + 32} 148h4"/>'
    b += text(186, 176, "Hash-chained audit", 8, "kt2")
    return svg(b, "Illustration: a checklist, an evidence CSV export and a chain of hashed audit records")


# ------------------------------------------------------------------ small scenes
def lost():
    b = plate(0, 320, 200)
    b += '<path class="k soft flowdash" d="M30 150C80 150 70 60 130 60S190 140 250 120" stroke-dasharray="3 5"/>'
    b += ribbon(240, 70, 1.05)
    b += card(36, 40, 90, 44) + text(48, 58, "404", 12, "kt", weight=700, mono=True) + lines(48, 68, [60], 0)
    return svg(b, "Illustration: a dotted path that wanders off to the Procera ribbon", 320, 200)


def envelope():
    """Envelope encryption: data key wraps the data, the workspace KEK wraps the data key."""
    b = plate(5, 320, 200)
    b += card(20, 46, 92, 110) + text(32, 66, "File chunk", 8, "kt2") + lines(32, 76, [64, 52, 68, 40, 58], 9)
    b += '<path class="k" d="M112 100h26"/><path class="k" d="M134 96l4 4-4 4"/>'
    b += card(142, 62, 70, 76) + '<path class="k" d="M160 92v-6a10 10 0 0 1 20 0v6"/><rect class="t3" x="155" y="92" width="30" height="24" rx="4"/>'
    b += text(177, 130, "data key", 8, "kt2", "middle")
    b += '<path class="k" d="M212 100h26"/><path class="k" d="M234 96l4 4-4 4"/>'
    b += card(242, 50, 60, 100) + ribbon(254, 66, 0.56) + text(272, 128, "KEK", 9, "kt", "middle", 700, mono=True)
    b += text(272, 141, "per workspace", 6.5, "kt2", "middle")
    return svg(b, "Diagram: a file chunk is encrypted with a data key, and the data key is wrapped by the workspace key-encryption key", 320, 200)


PILLAR_ART = {"design": design, "execute": execute, "govern": govern, "improve": improve}
PERSONA_ART = {"hr": hr, "manager": manager, "employee": employee, "it": it_security,
               "finance": finance, "compliance": compliance}
SCENES = {"lost": lost, "envelope": envelope, **PILLAR_ART, **PERSONA_ART}
