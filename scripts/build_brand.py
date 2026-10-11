"""Render Procera's identity from one geometric master and the web app's Geist font.

After npm ci in web/: pip install cairosvg pillow fonttools brotli
Run from any directory: python3 scripts/build_brand.py
"""
from pathlib import Path

import cairosvg
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
BRAND = ROOT / "assets/brand"
PUBLIC = ROOT / "web/public"
FONT = ROOT / "web/node_modules/@fontsource-variable/geist/files/geist-latin-wght-normal.woff2"
RED = "#EE4136"
INK = "#0F172A"
LIGHT = "#F8FAFC"
# Broad stem, open counter and stepped terminal remain legible at favicon sizes.
MARK = "M4 4H18C24 4 28 8 28 14V17H25V19C23 22 21 23 18 23H11V28H4ZM11 10V17H18C21 17 22 16 22 14C22 11 21 10 18 10Z"
FAVICON_SIZES = (16, 32, 48, 64, 128, 256, 512)


def mark_path(color=RED):
    return f'<path fill="{color}" fill-rule="evenodd" d="{MARK}"/>'


def symbol(color=RED, background=None):
    tile = f'<rect width="32" height="32" rx="7" fill="{background}"/>' if background else ""
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">{tile}{mark_path(color)}</svg>'


def render(svg, destination, width, height=None):
    destination.parent.mkdir(parents=True, exist_ok=True)
    cairosvg.svg2png(
        bytestring=svg.encode(), write_to=str(destination),
        output_width=width, output_height=height or width,
    )


class Lettering:
    """Outline the npm-shipped font so exports do not depend on machine fonts."""

    def __init__(self):
        self.fonts = {
            weight: instantiateVariableFont(TTFont(FONT), {"wght": weight}, inplace=False)
            for weight in (400, 650)
        }

    def draw(self, text, x, baseline, size, color, weight=400, center=False):
        font = self.fonts[weight]
        glyphs, cmap = font.getGlyphSet(), font.getBestCmap()
        scale = size / font["head"].unitsPerEm
        width = sum(font["hmtx"][cmap[ord(c)]][0] for c in text) * scale
        cursor, paths = x - width / 2 if center else x, []
        for character in text:
            name = cmap[ord(character)]
            pen = SVGPathPen(glyphs)
            glyphs[name].draw(pen)
            paths.append(
                f'<path fill="{color}" transform="translate({cursor:.4f},{baseline}) '
                f'scale({scale},{-scale})" d="{pen.getCommands()}"/>'
            )
            cursor += font["hmtx"][name][0] * scale
        return "".join(paths)


def lockup(lettering, dark=False, stacked=False, background=None):
    width, height = (480, 320) if stacked else (640, 160)
    color, muted = (LIGHT, "#CBD5E1") if dark else (INK, "#475569")
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}">']
    if background:
        parts.append(f'<rect width="{width}" height="{height}" fill="{background}"/>')
    tx, ty, scale = (192, 16, 3) if stacked else (16, 24, 3.5)
    parts.append(f'<g transform="translate({tx},{ty}) scale({scale})">{mark_path()}</g>')
    x, y = (240, 200) if stacked else (150, 94)
    parts.append(lettering.draw("Procera", x, y, 64, color, 650, stacked))
    parts.append(lettering.draw("Design · Execute · Govern · Improve", x, y + 40, 19, muted, center=stacked))
    return "".join(parts) + "</svg>", width, height


def banner(lettering):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1523" height="175" viewBox="0 0 1523 175">'
        f'<rect width="1523" height="175" fill="{INK}"/>'
        f'<g transform="translate(40,30) scale(3.5)">{mark_path()}</g>'
        + lettering.draw("Procera", 190, 90, 56, LIGHT, 650)
        + lettering.draw("Turn processes into progress.", 600, 80, 28, LIGHT)
        + lettering.draw("Design · Execute · Govern · Improve", 600, 120, 19, "#CBD5E1")
        + "</svg>"
    )


def main():
    master = symbol()
    for destination in (BRAND / "logo/procera-symbol.svg", PUBLIC / "procera-symbol.svg", PUBLIC / "favicon.svg"):
        destination.write_text(master)
    for destination in (BRAND / "logo/procera-symbol.png", PUBLIC / "procera-symbol.png"):
        render(master, destination, 256)
    render(symbol(background=LIGHT), BRAND / "logo/procera-symbol-on-light.png", 256)

    lettering = Lettering()
    variants = (
        ("horizontal", False, False, LIGHT),
        ("horizontal-transparent", False, False, None),
        ("horizontal-dark", True, False, None),
        ("stacked", False, True, LIGHT),
        ("stacked-transparent", False, True, None),
        ("stacked-dark", True, True, None),
    )
    for name, dark, stacked, background in variants:
        svg, width, height = lockup(lettering, dark, stacked, background)
        (BRAND / f"logo/procera-logo-{name}.svg").write_text(svg)
        render(svg, BRAND / f"logo/procera-logo-{name}.png", width, height)

    for size in FAVICON_SIZES:
        render(master, BRAND / f"favicon/favicon-{size}.png", size)
    with Image.open(BRAND / "favicon/favicon-256.png") as image:
        image.save(BRAND / "favicon/favicon.ico", sizes=[(size, size) for size in FAVICON_SIZES if size <= 256])
    (PUBLIC / "favicon.ico").write_bytes((BRAND / "favicon/favicon.ico").read_bytes())

    for name, background, color in (
        ("light", LIGHT, RED), ("dark", INK, RED),
        ("dark-alt", "#172033", RED), ("red", RED, "#FFFFFF"),
    ):
        render(symbol(color, background), BRAND / f"icons/app-icon-{name}.png", 512)
    render(banner(lettering), BRAND / "marketing/procera-banner.png", 1523, 175)
    marketing_assets(lettering)
    print("Generated SVG masters, Geist lockups, app icons, banner and favicon set.")


def marketing_assets(lettering):
    def canvas(width, height, background=LIGHT):
        return f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="100%" height="100%" fill="{background}"/>'

    def save(name, svg, width, height):
        svg += '</svg>'
        (BRAND / f'marketing/{name}.svg').write_text(svg)
        render(svg, BRAND / f'marketing/{name}.png', width, height)

    for name, color in [('black', INK), ('white', '#FFFFFF')]:
        (BRAND / f'logo/procera-symbol-{name}.svg').write_text(symbol(color))
        render(symbol(color), BRAND / f'logo/procera-symbol-{name}.png', 512)
        svg, width, height = lockup(lettering, dark=name == 'white')
        svg = svg.replace(RED, color).replace('#CBD5E1', color).replace('#475569', color)
        (BRAND / f'logo/procera-logo-horizontal-{name}.svg').write_text(svg)
        render(svg, BRAND / f'logo/procera-logo-horizontal-{name}.png', width, height)

    dark_banner = banner(lettering)
    (BRAND / 'marketing/procera-banner.svg').write_text(dark_banner)
    light_banner = dark_banner.replace(f'fill="{INK}"', 'fill="#FFFFFF"', 1).replace(f'fill="{LIGHT}"', f'fill="{INK}"').replace('fill="#CBD5E1"', 'fill="#475569"')
    (BRAND / 'marketing/procera-banner-light.svg').write_text(light_banner)
    render(light_banner, BRAND / 'marketing/procera-banner-light.png', 1523, 175)

    social = canvas(1280, 640, INK)
    social += f'<g transform="translate(66,66) scale(4)">{mark_path()}</g>'
    social += lettering.draw('Procera', 245, 163, 86, LIGHT, 650)
    social += lettering.draw('Turn processes', 80, 335, 70, LIGHT, 650)
    social += lettering.draw('into progress.', 80, 420, 70, LIGHT, 650)
    social += lettering.draw('Design · Execute · Govern · Improve', 84, 535, 27, '#CBD5E1')
    save('procera-social-preview', social, 1280, 640)

    copy_cards = {
        'procera-tagline': ['Turn processes into progress.', 'Design · Execute · Govern · Improve'],
        'procera-short-description': ['Open-source process management.', 'Design, automate, govern and improve', 'your business processes in one place.'],
        'procera-long-description': ['Policies, procedures and approvals.', 'Governed documents, acknowledgements,', 'announcements, files and audit trails.', 'Built for teams. Designed for clarity.'],
    }
    for name, lines in copy_cards.items():
        svg = canvas(1200, 500)
        svg += f'<g transform="translate(52,40) scale(2)">{mark_path()}</g>'
        svg += lettering.draw('Procera', 140, 90, 42, INK, 650)
        for index, line in enumerate(lines):
            svg += lettering.draw(line, 64, 210 + index * 60, 44 if index == 0 else 32, INK, 650 if index == 0 else 400)
        save(name, svg, 1200, 500)

    svg = canvas(1200, 420)
    svg += lettering.draw('Brand palette', 48, 72, 38, INK, 650)
    palette = [('Brand', RED), ('Accent', '#C72D26'), ('Charcoal', INK), ('Slate', '#64748B'), ('Paper', LIGHT), ('White', '#FFFFFF')]
    for index, (name, color) in enumerate(palette):
        x = 48 + index * 190
        svg += f'<rect x="{x}" y="115" width="164" height="150" rx="12" fill="{color}" stroke="#CBD5E1"/>'
        svg += lettering.draw(name, x, 310, 23, INK, 650) + lettering.draw(color, x, 348, 21, '#475569')
    save('procera-color-palette', svg, 1200, 420)

    svg = canvas(1200, 420)
    svg += lettering.draw('Small-size clarity', 48, 72, 38, INK, 650)
    for index, size in enumerate([16, 24, 32, 48, 64, 128]):
        x = 70 + index * 190
        svg += f'<g transform="translate({x},{180-size/2}) scale({size/32})">{mark_path()}</g>'
        svg += lettering.draw(f'{size}px', x, 300, 23, INK)
    save('procera-favicon-reference', svg, 1200, 420)

    svg = canvas(1200, 420)
    svg += lettering.draw('Monochrome identity', 48, 72, 38, INK, 650)
    svg += f'<g transform="translate(52,150) scale(4)">{mark_path(INK)}</g>'
    svg += lettering.draw('Procera', 230, 240, 74, INK, 650)
    svg += f'<rect x="640" y="120" width="510" height="220" rx="12" fill="{INK}"/>'
    svg += f'<g transform="translate(655,160) scale(3.5)">{mark_path("#FFFFFF")}</g>'
    svg += lettering.draw('Procera', 800, 245, 65, '#FFFFFF', 650)
    save('procera-monochrome-logos', svg, 1200, 420)

    # One review sheet displays the same masters used by the product and README.
    svg = canvas(1200, 850)
    svg += lettering.draw('Procera · Identity v2', 56, 70, 30, INK, 650)
    svg += f'<g transform="translate(50,112) scale(4)">{mark_path()}</g>'
    svg += lettering.draw('Procera', 220, 208, 88, INK, 650)
    svg += lettering.draw('Turn processes into progress.', 58, 305, 42, INK)
    svg += f'<rect x="48" y="364" width="1104" height="210" rx="16" fill="{INK}"/>'
    svg += f'<g transform="translate(78,400) scale(3.5)">{mark_path()}</g>'
    svg += lettering.draw('Procera', 242, 480, 72, LIGHT, 650)
    svg += lettering.draw('Design · Execute · Govern · Improve', 246, 530, 25, '#CBD5E1')
    svg += lettering.draw('App icons', 56, 630, 26, INK, 650)
    for index, (background, color) in enumerate([(LIGHT, RED), (INK, RED), (RED, '#FFFFFF')]):
        svg += f'<g transform="translate({56+index*125},660)"><rect width="88" height="88" rx="20" fill="{background}" stroke="#CBD5E1"/><g transform="scale(2.75)">{mark_path(color)}</g></g>'
    svg += lettering.draw('Favicon · 16 / 24 / 32px', 570, 630, 26, INK, 650)
    for index, size in enumerate([16, 24, 32]):
        svg += f'<g transform="translate({578+index*95},693) scale({size/32})">{mark_path()}</g>'
    save('procera-brand-overview', svg, 1200, 850)
    render(symbol(background=LIGHT), PUBLIC / 'apple-touch-icon.png', 180)
    for size in (192, 512):
        render(symbol(background=LIGHT), BRAND / f'icons/app-icon-{size}.png', size)
        (PUBLIC / f'icon-{size}.png').write_bytes((BRAND / f'icons/app-icon-{size}.png').read_bytes())


if __name__ == "__main__":
    main()
