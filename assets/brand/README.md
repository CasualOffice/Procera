# Procera brand assets

Status: **v2 (2026-10-12): vector masters.** Every mark is built from one geometry by [`vector/build.py`](vector/build.py), and the wordmark is outlined from Geist (SIL OFL 1.1, the product UI font). The PNGs are exported from those SVGs. Trademark clearance is still to do (see [Production caveats](#production-caveats)). How the brand is applied in the product UI is normative in [docs/05 §7.1.1](../../docs/05-UX-AND-DESIGN.md).

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo/procera-logo-horizontal-dark.png">
    <img src="logo/procera-logo-horizontal-transparent.png" alt="Procera — Design · Execute · Govern · Improve" width="420">
  </picture>
</p>

## Brand copy

| | |
|---|---|
| **Name** | Procera |
| **Tagline** | Turn processes into progress. |
| **Supporting line** | Design · Execute · Govern · Improve |
| **Short description** | Procera is a modern, open-source business process management platform that helps organizations design, automate, govern, and continuously improve their business processes, end to end. |
| **Long description** | Procera combines process modeling, workflow automation, documentation, case management, compliance, and insights into a single, easy-to-use platform. It empowers teams to create and run efficient processes, maintain operational control, ensure compliance, and drive continuous improvement, with a focus on simplicity, flexibility, and open-source freedom. |
| **GitHub one-liner** | Open-source business process management for modeling, automating, governing, and improving workflows. |

**Pillars**

| Pillar | Meaning | Product areas |
|---|---|---|
| **Design** | Model and document processes | Library, SOPs, policies, designer, templates |
| **Execute** | Automate work across people and systems | Requests, approvals, forms, e-signing, announcements, integrations |
| **Govern** | Ensure compliance, control and visibility | Lifecycle and SoD, roles, audit trail, investigations, retention |
| **Improve** | Gain insights and continuously optimize | Dashboards, SLAs, process health, improvement proposals |

Writing rules:
- Always write **Procera**: capital P, never "PROCERA" in running text and never "ProCera".
- Product areas use plain words ("Policies", "My Work"), never "Procera Policies™".
- No superlatives we can't prove. The compliance wording rules in [docs/14](../../docs/14-DATA-GOVERNANCE-AND-COMPLIANCE.md) apply to marketing too.

## Colour palette

| Name | Hex | Role (see docs/05 §7.1.1) | On #FFFFFF | On #0F172A |
|---|---|---|---|---|
| Flamingo Red (Primary) | `#EE4136` | `brand`: logo, focus, active state, large display text | 3.86 (UI/large text only) | 4.63 |
| Coral (Primary Light) | `#FF6B5A` | Dark-theme links / accent text | 2.80 ✗ | 6.38 |
| Deep Red (Primary Dark) | `#C72D26` | Light-theme links; optional filled accent button | 5.48 | 3.26 ✗ |
| Charcoal (Text Primary) | `#0F172A` | Text, primary buttons, dark background | 17.85 | — |
| Slate (Text Secondary) | `#64748B` | Secondary text on white / light gray only | 4.76 | 3.75 ✗ |
| Light Gray (Background) | `#F8FAFC` | Page background; dark-theme text | — | 17.06 |
| White (Surface) | `#FFFFFF` | Panels, cards, light logo backgrounds | — | 17.85 |

✗ = below 4.5:1, so not allowed for normal-size text in that pairing.

**Key rule:** brand red is the brand signal, not the action colour. Primary buttons are charcoal. Destructive actions use a separate crimson (`#B42318`) together with an icon and words. The rationale is in docs/05 §7.1.1.

## Files

| Path | What | Use |
|---|---|---|
| `vector/*.svg` | **Masters.** Symbol (colour, small/flat, white, ink), horizontal lockups with and without the supporting line (light and dark), stacked lockups, app-icon tiles, banner | Use these wherever SVG works; rebuild with `python3 assets/brand/vector/build.py` |
| `logo/procera-logo-horizontal.png` | Horizontal lockup with supporting line, light background | Docs and light backgrounds |
| `logo/procera-logo-horizontal-transparent.png` | Same, transparent | Light theme (README, site) |
| `logo/procera-logo-horizontal-dark.png` | White wordmark, transparent | Dark theme |
| `logo/procera-logo-stacked*.png` | Stacked lockup (light / transparent / dark) | Square placements, splash, login |
| `logo/procera-symbol.png` | P-ribbon symbol, transparent 512² (from `vector/procera-symbol.svg`) | App shell, avatars, small placements |
| `logo/procera-symbol-on-light.png` | Symbol on light background | Where transparency isn't supported |
| `icons/app-icon-{light,dark,dark-alt,red}.png` | App icon tiles, padded square | PWA/manifest previews, store mock-ups (re-render from vector for production sizes) |
| `favicon/favicon.ico`, `favicon/favicon-{16,32,48,64,128,256,512}.png` | Favicon set | Web app and docs site (`<link rel="icon">`, PWA manifest 192/512 to be generated from vector) |
| `marketing/procera-banner.png` | 1523×175 dark banner with tagline | GitHub README header, social and site hero |
| `marketing/procera-tagline.png`, `-short-description.png`, `-long-description.png` | Copy cards | Presentations |
| `marketing/procera-color-palette.png`, `-monochrome-logos.png`, `-favicon-reference.png` | Board reference sheets | Reference only |
| `source/` | Untouched originals from `Procera_Brand_Assets.zip` (incl. `BRAND_COPY.md`) | Provenance; don't edit |

The PNGs in `logo/`, `icons/`, `favicon/` and `marketing/procera-banner.png` are exported from `vector/` (2× sizes, crisp edges, no halo). Favicons at 16 and 32 px use the flat small symbol, where the fold shading would turn to mush. `source/` keeps the original concept board for provenance.

## Logo usage

- **Clear space:** at least half the symbol's height on all sides.
- **Minimum size:** symbol 16 px. Horizontal lockup 120 px wide. Drop the supporting line below 240 px.
- **Backgrounds:** use the colour logo on white, `#F8FAFC` or light neutrals, and the dark variant on `#0F172A` or dark neutrals. Use monochrome where colour isn't possible: print, embossing, a single-colour screen.
- **Don't** recolour the symbol, stretch it, rotate it, add effects or shadows, place it on busy photos, or rebuild the wordmark in another font.

## Production caveats

1. **Vector masters: done (v2).** `vector/` holds the SVG masters, including a flat symbol for 16–32 px. A professional type and mark review before GA is still recommended (BPMS-034).
2. **Trademark and domain clearance** for "Procera" in target markets has **not** been performed. Tracked as **OQ-01** in [docs/22](../../docs/22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md). Other companies and products already use "Procera", so a search is required before public launch.
3. The palette matches the vector masters: brand red `#EE4136`, shading `#F24C3D` → `#E5392E` on the band and `#AE231D` → `#F24C3D` on the fold.
4. Licence: brand assets are **not** covered by the Apache-2.0 code licence. They are trademarks of CasualOffice. Use follows [GOVERNANCE.md §7](../../GOVERNANCE.md#7-trademark).
