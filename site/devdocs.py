"""The developer docs (site/developers.html and docs-*.html): self-hosting, configuration and the API.

Self-hosting pages render docs/guides (one source of truth, shared with Get started); the API pages
render site/src/developers/*.md; the API reference is generated from the server's router by
site/scripts/api_reference.py, and the build fails if an endpoint has no summary, so the reference
can't drift from the code. Rendering reuses build.py's Markdown subset.
"""

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "scripts"))
import api_reference  # noqa: E402

# (page, title, source relative to the repo or None for the reference, group, one-line description)
DOCS = [
    ("docs-install", "Install with Docker Compose", "docs/guides/install-docker-compose.md", "Self-hosting",
     "Run the database, the server and the web app on one machine."),
    ("docs-setup", "First-time setup", "docs/guides/first-time-setup.md", "Self-hosting",
     "From the one-time setup token to a workspace with people in it."),
    ("docs-configuration", "Configuration reference", "docs/guides/configuration.md", "Self-hosting",
     "Every environment variable and command-line option."),
    ("docs-api", "API overview", "site/src/developers/api-overview.md", "API",
     "Base URL, authentication with API tokens, and a first request."),
    ("docs-conventions", "Conventions", "site/src/developers/conventions.md", "API",
     "Errors, safe retries, concurrent edits, lists and time."),
    ("docs-reference", "API reference", None, "API",
     "Every endpoint, generated from the server's router."),
    ("docs-webhooks", "Webhooks", "site/src/developers/webhooks.md", "Extend",
     "Signed events delivered to your systems, with retries and a delivery log."),
    ("docs-plugins", "Plugins", "site/src/developers/plugins.md", "Extend",
     "Connectors and extensions (planned)."),
]
# Guide file stem → its docs page, for links between guides.
PAGES = {Path(src).stem: page for page, _, src, *_ in DOCS if src}
VERB = {"GET": "get", "POST": "post", "PUT": "put", "PATCH": "patch", "DELETE": "delete"}


def sidebar(b, active):
    groups = {}
    for page, title, _, group, _ in DOCS:
        groups.setdefault(group, []).append((page, title))
    cur = ' aria-current="page"'
    out = ['<nav class="docs-nav" aria-label="Developer docs">',
           f'<a class="docs-home" href="developers.html"{cur if active == "developers" else ""}>Developers</a>']
    for group, items in groups.items():
        links = "".join(f'<li><a href="{p}.html"{cur if p == active else ""}>{b.esc(t)}</a></li>' for p, t in items)
        out.append(f'<p class="docs-group">{b.esc(group)}</p><ul>{links}</ul>')
    out.append("</nav>")
    return "".join(out)


def on_page(b, body, level=3):
    heads = re.findall(rf'<h{level} id="([^"]+)">(.*?)</h{level}>', body)
    if len(heads) < 3:
        return ""
    items = "".join(f'<li><a href="#{hid}">{re.sub(r"<[^>]+>", "", t)}</a></li>' for hid, t in heads)
    return f'<aside class="docs-toc" aria-label="On this page"><p>On this page</p><ul>{items}</ul></aside>'


def layout(b, page, title, lede, body, source, toc_level=3):
    edit = (f'<p class="docs-edit">Source: <a href="{b.GITHUB}/blob/main/{source}">{source}</a></p>'
            if source else "")
    menu = sidebar(b, page).replace('aria-label="Developer docs"', 'aria-label="Developer docs (menu)"', 1)
    return (f'<div class="docs wrap"><details class="docs-menu"><summary>Developer docs</summary>{menu}</details>'
            f'{sidebar(b, page)}<article class="docs-body guide prose">'
            f'<p class="eyebrow">Developers</p><h1>{b.esc(title)}</h1><p class="lede">{b.esc(lede)}</p>'
            f'{body}{edit}</article>{on_page(b, body, toc_level)}</div>')


def reference_body(b, ref):
    total = sum(len(a["routes"]) for a in ref["areas"])
    parts = [f'<p>{total} operations in {len(ref["areas"])} areas, generated from the server\'s router. Paths are '
             'relative to your server (for example <code>https://procera.example.com</code>); authenticate as described '
             'in the <a href="docs-api.html">API overview</a>. The <a href="openapi.json">OpenAPI 3.1 document</a> has the '
             'same paths and summaries for tools. Request and response bodies are described in words until typed '
             'schemas land.</p>']
    for a in ref["areas"]:
        hid = b.slugify(a["name"])
        ops = []
        for r in a["routes"]:
            rid = b.slugify(f'{r["method"]} {r["path"]}'.replace("/", " ").replace("{", "").replace("}", ""))
            notes = (f'<p class="op-notes">Audited as {", ".join(f"<code>{b.esc(e)}</code>" for e in r["audited"])}</p>'
                     if r.get("audited") else "")
            ops.append(f'<li class="op" id="{rid}"><p class="op-head"><span class="verb verb-{VERB[r["method"]]}">'
                       f'{r["method"]}</span><code class="op-path">{b.esc(r["path"])}</code></p>'
                       f'<p class="op-sum">{b.md_inline(r["summary"], "reference")}</p>{notes}</li>')
        intro = f'<p class="area-intro">{b.md_inline(a["intro"], "reference")}</p>' if a["intro"] else ""
        parts.append(f'<section class="area" aria-labelledby="{hid}"><h2 id="{hid}">{b.esc(a["name"])}</h2>'
                     f'{intro}<ul class="ops" aria-label="{b.esc(a["name"])} endpoints">{"".join(ops)}</ul></section>')
    return "".join(parts)


def references(root):
    """The reference and the OpenAPI document; refuses to build when an endpoint has no summary."""
    ref = api_reference.collect()
    missing = [f'{r["method"]} {r["path"]}' for a in ref["areas"] for r in a["routes"] if not r["summary"]]
    if missing:
        raise SystemExit("build: endpoints without a /// summary (they appear in the API reference):\n  "
                         + "\n  ".join(missing))
    data = root / "site" / "data"
    (data / "api.json").write_text(json.dumps(ref, indent=1, ensure_ascii=False) + "\n")
    return ref, api_reference.openapi(ref, "0.4.0-dev")


def build_docs(b, wrap_page):
    """Writes the docs pages; `wrap_page(page, title, description, body)` adds head, header and footer."""
    root = b.ROOT
    ref, spec = references(root)
    built = []
    b.DOCS_PAGES.update(PAGES)
    try:
        for page, title, src, _, lede in DOCS:
            if src:
                md = (root / src).read_text()
                md = re.sub(r"\A#\s+.*\n", "", md)  # the title is the page's h1
                body = b.md_to_html(md, Path(src).stem)
                html_ = layout(b, page, title, lede, body, src)
            else:
                html_ = layout(b, page, title, lede, reference_body(b, ref), None, toc_level=2)
            (b.DIST / f"{page}.html").write_text(wrap_page(page, title, lede, html_))
            built.append(f"{page}.html")
    finally:
        b.DOCS_PAGES.clear()
    (b.DIST / "openapi.json").write_text(json.dumps(spec, indent=1, ensure_ascii=False) + "\n")
    return built
