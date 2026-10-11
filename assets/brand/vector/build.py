#!/usr/bin/env python3
"""Builds the Procera vector logo set (SVG) from one geometry and Geist outlines.

The wordmark and supporting line are glyph outlines (no font needed to display them), taken from
Geist (SIL OFL 1.1, the product UI font) via fontTools. Run from the repo root:

    python3 assets/brand/vector/build.py

Needs `pip install fonttools brotli` and web/node_modules (for the Geist file).
"""
from pathlib import Path

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parents[3]
FONT = ROOT / 'web/node_modules/@fontsource-variable/geist/files/geist-latin-wght-normal.woff2'
OUT = Path(__file__).resolve().parent

INK, LIGHT, SLATE, SLATE_DARK, RED, RED_DARK = '#0F172A', '#F8FAFC', '#64748B', '#94A3B8', '#EE4136', '#C72D26'

# The P-ribbon on a 64-unit grid: the top band curls into the bowl (r 18 outside, 5 inside); the
# ribbon folds out from under the bowl and runs down-left as the stem (its fold carries the shade).
BAND = 'M15 8H38A18 18 0 0 1 38 44H31V31H38A5 5 0 0 0 38 21H15A5 5 0 0 1 10 16V13A5 5 0 0 1 15 8Z'
STEM = ('M31 31V44C26.6 47.4 21.8 51.8 17.6 56.2C15.8 58.1 13.6 59.4 11.4 59.4C9.3 59.4 8 57.8 8 55.8'
        'V49.6C8 46.5 9.4 43.6 11.9 41.7L22.8 33.4C25.1 31.8 28 31 31 31Z')
VIS = (8.0, 8.0, 56.0, 59.4)  # visible bounds of the symbol inside its 64 box
DEFS = ('<linearGradient id="pb" x1="10" y1="8" x2="56" y2="44" gradientUnits="userSpaceOnUse">'
        '<stop offset="0" stop-color="#F24C3D"/><stop offset="1" stop-color="#E5392E"/></linearGradient>'
        '<linearGradient id="pf" x1="31" y1="36" x2="12" y2="56" gradientUnits="userSpaceOnUse">'
        '<stop offset="0" stop-color="#AE231D"/><stop offset=".4" stop-color="#D9352B"/>'
        '<stop offset="1" stop-color="#F24C3D"/></linearGradient>')


def sym(band=None, stem=None):
    if band:
        return f'<path fill="{stem or band}" d="{STEM}"/><path fill="{band}" d="{BAND}"/>'
    return f'<path fill="url(#pf)" d="{STEM}"/><path fill="url(#pb)" d="{BAND}"/>'


def svg(w, h, body, title='Procera', pad=0.0):
    """An SVG document; `pad` adds clear space on every side (lockups use it)."""
    if pad:
        body, w, h = f'<g transform="translate({pad:g} {pad:g})">{body}</g>', w + 2 * pad, h + 2 * pad
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:g} {h:g}" role="img" aria-label="{title}">'
            f'<title>{title}</title><defs>{DEFS}</defs>{body}</svg>\n')

PAD = 12  # clear space around lockups


def outline(weight, text, tracking):
    font = instancer.instantiateVariableFont(TTFont(FONT), {'wght': weight})
    glyphs, cmap, upm = font.getGlyphSet(), font.getBestCmap(), font['head'].unitsPerEm
    pen, x = SVGPathPen(glyphs), 0.0
    for ch in text:
        g = glyphs[cmap[ord(ch)]]
        g.draw(TransformPen(pen, (1, 0, 0, -1, x, 0)))
        x += g.width + tracking * upm
    return pen.getCommands(), x - tracking * upm, font['OS/2'].sCapHeight


WORD, WORD_W, WORD_CAP = outline(640, 'Procera', -0.025)
TAG, TAG_W, TAG_CAP = outline(560, 'DESIGN · EXECUTE · GOVERN · IMPROVE', 0.16)
LINE, LINE_W, LINE_CAP = outline(460, 'Turn processes into progress.', -0.01)


def banner():
    """The 1520×176 dark README/social banner: the lockup left, the tagline right."""
    w, h, s = 1520, 176, 96 / 64
    body = f'<rect width="{w}" height="{h}" fill="{INK}"/>'
    oy = (h - 96) / 2
    body += f'<g transform="translate(56 {oy:g}) scale({s})">{sym()}</g>'
    centre = oy + (VIS[1] + VIS[3]) / 2 * s
    k = 50 / WORD_CAP
    body += placed(WORD, 56 + VIS[2] * s + 20, centre + 25, k, LIGHT)
    kl = 30 / LINE_CAP
    body += placed(LINE, w - 64 - LINE_W * kl, centre + 15, kl, '#CBD5E1')
    return svg(w, h, body, 'Procera: Turn processes into progress.')


def placed(d, x, baseline, k, color):
    return f'<path fill="{color}" transform="translate({x:.2f} {baseline:.2f}) scale({k:.5f})" d="{d}"/>'


def horizontal(text, tagline=None):
    s = 96 / 64
    body = f'<g transform="scale({s})">{sym()}</g>'
    centre = (VIS[1] + VIS[3]) / 2 * s
    cap = 50
    x0 = VIS[2] * s + 20
    k = cap / WORD_CAP
    body += placed(WORD, x0, centre + cap / 2, k, text)
    w, h = x0 + WORD_W * k + 4, 96.0
    if tagline:  # full width, under the symbol and the wordmark
        kt = (w - VIS[0] * s - 4) / TAG_W
        body += placed(TAG, VIS[0] * s, h + 10 + TAG_CAP * kt, kt, tagline)
        h += 14 + TAG_CAP * kt
    return svg(round(w, 1), round(h, 1), body, 'Procera, Design · Execute · Govern · Improve' if tagline else 'Procera', PAD)


def stacked(text):
    cap, s = 44, 120 / 64
    ww = WORD_W * cap / WORD_CAP
    w = max(ww, (VIS[2] - VIS[0]) * s) + 8
    sx = (w - (VIS[2] - VIS[0]) * s) / 2 - VIS[0] * s
    base = VIS[3] * s + 22 + cap
    body = f'<g transform="translate({sx:.2f} 0) scale({s})">{sym()}</g>' + placed(WORD, (w - ww) / 2, base, cap / WORD_CAP, text)
    return svg(round(w, 1), round(base + 12, 1), body, pad=PAD)


def tile(bg, band=None, stem=None):
    s = 344 / 64
    ox = (512 - (VIS[2] - VIS[0]) * s) / 2 - VIS[0] * s
    oy = (512 - (VIS[3] - VIS[1]) * s) / 2 - VIS[1] * s
    return svg(512, 512, f'<rect width="512" height="512" rx="112" fill="{bg}"/>'
                         f'<g transform="translate({ox:.2f} {oy:.2f}) scale({s})">{sym(band, stem)}</g>')


FILES = {
    'procera-symbol.svg': svg(64, 64, sym()),
    'procera-symbol-small.svg': svg(64, 64, sym(RED, RED_DARK)),  # ≤ 32 px and favicons: flat
    'procera-symbol-white.svg': svg(64, 64, sym('#FFFFFF', '#FFFFFF')),
    'procera-symbol-ink.svg': svg(64, 64, sym(INK, INK)),
    'procera-logo-horizontal.svg': horizontal(INK),
    'procera-logo-horizontal-dark.svg': horizontal(LIGHT),
    'procera-logo-horizontal-tagline.svg': horizontal(INK, SLATE),
    'procera-logo-horizontal-tagline-dark.svg': horizontal(LIGHT, SLATE_DARK),
    'procera-logo-stacked.svg': stacked(INK),
    'procera-logo-stacked-dark.svg': stacked(LIGHT),
    'app-icon-light.svg': tile('#FFFFFF'),
    'app-icon-dark.svg': tile(INK),
    'app-icon-red.svg': tile(RED, '#FFFFFF', '#FFD9D5'),
    'procera-banner.svg': banner(),
}

if __name__ == '__main__':
    for name, data in FILES.items():
        (OUT / name).write_text(data)
    print('\n'.join(sorted(FILES)))
