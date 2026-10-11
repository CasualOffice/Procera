//! Workspace accent → the shades the UI may use (docs/05 §7, docs/20 §2.1 `branding.accent`:
//! "contrast ≥ 4.5:1 enforced, auto-derived text-safe shade"). Any colour is accepted; what reaches
//! the screen is moved toward black (light theme) or white (dark theme) just far enough to pass:
//! link text and filled-button labels 4.5:1, the focus ring 3:1 (WCAG 2.2 1.4.3, 1.4.11).

use serde::Serialize;

/// Light theme surfaces the accent must read on (tokens.css: surface, bg, surface-muted).
const LIGHT: [&str; 3] = ["#FFFFFF", "#F7F7F5", "#F1F1EF"];
/// Dark theme surfaces (surface, bg, surface-muted).
const DARK: [&str; 3] = ["#191919", "#202020", "#2A2A2A"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shades {
    /// Links and text actions (≥ 4.5:1 on every surface).
    pub text: String,
    /// Filled accent button background (its label is white in light, near-black in dark: ≥ 4.5:1).
    pub strong: String,
    pub strong_hover: String,
    /// Focus ring (≥ 3:1 on every surface).
    pub focus: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Branding {
    /// As configured.
    pub accent: String,
    pub light: Shades,
    pub dark: Shades,
    /// The configured colour itself didn't meet the contrast rules and was adjusted for text.
    pub adjusted: bool,
}

fn rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| f64::from(u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0));
    (p(0), p(2), p(4))
}

fn hex((r, g, b): (f64, f64, f64)) -> String {
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", c(r), c(g), c(b))
}

fn luminance(hex: &str) -> f64 {
    let (r, g, b) = rgb(hex);
    let lin = |c: f64| {
        let c = c / 255.0;
        if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG contrast ratio between two colours.
pub fn contrast(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

fn mix(c: &str, toward: (f64, f64, f64), t: f64) -> String {
    let (r, g, b) = rgb(c);
    hex((r + (toward.0 - r) * t, g + (toward.1 - g) * t, b + (toward.2 - b) * t))
}

/// The closest shade of `c` (moving toward `toward`) whose contrast with every one of `on` is ≥ `min`.
fn reach(c: &str, toward: (f64, f64, f64), on: &[&str], min: f64) -> String {
    for step in 0..=100 {
        let s = mix(c, toward, f64::from(step) / 100.0);
        if on.iter().all(|bg| contrast(&s, bg) >= min) {
            return s;
        }
    }
    hex(toward)
}

/// Derives every shade from the configured accent (`#RRGGBB`, already validated).
pub fn derive(accent: &str) -> Branding {
    const BLACK: (f64, f64, f64) = (0.0, 0.0, 0.0);
    const WHITE: (f64, f64, f64) = (255.0, 255.0, 255.0);
    let light_text = reach(accent, BLACK, &LIGHT, 4.5);
    let dark_text = reach(accent, WHITE, &DARK, 4.5);
    let light = Shades {
        // White labels on the filled button need the same 4.5:1 as text on white.
        strong: reach(accent, BLACK, &["#FFFFFF"], 4.5),
        strong_hover: mix(&reach(accent, BLACK, &["#FFFFFF"], 4.5), BLACK, 0.15),
        focus: reach(accent, BLACK, &LIGHT, 3.0),
        text: light_text.clone(),
    };
    let dark = Shades {
        strong: reach(accent, WHITE, &["#191919"], 4.5),
        strong_hover: mix(&reach(accent, WHITE, &["#191919"], 4.5), WHITE, 0.15),
        focus: reach(accent, WHITE, &DARK, 3.0),
        text: dark_text,
    };
    Branding {
        accent: accent.to_uppercase(),
        adjusted: !light_text.eq_ignore_ascii_case(accent),
        light,
        dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn brand_red_needs_a_darker_text_shade_and_keeps_its_focus_colour() {
        let b = derive("#EE4136");
        assert!(b.adjusted, "#EE4136 is 3.7:1 on white, below 4.5");
        assert!(contrast(&b.light.text, "#FFFFFF") >= 4.5 && contrast(&b.light.text, "#F1F1EF") >= 4.5);
        assert_eq!(b.light.focus, "#EE4136", "already 3:1 on light surfaces");
    }

    #[test]
    fn a_dark_accent_is_used_as_is_and_a_pale_one_is_darkened() {
        assert!(!derive("#1F4E79").adjusted);
        let y = derive("#FFE14D");
        assert!(y.adjusted && contrast(&y.light.text, "#FFFFFF") >= 4.5);
        assert!(contrast(&y.dark.text, "#2A2A2A") >= 4.5);
    }

    proptest! {
        #[test]
        fn every_shade_meets_its_contrast_rule(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255) {
            let d = derive(&format!("#{r:02X}{g:02X}{b:02X}"));
            for bg in LIGHT { prop_assert!(contrast(&d.light.text, bg) >= 4.5); prop_assert!(contrast(&d.light.focus, bg) >= 3.0); }
            for bg in DARK { prop_assert!(contrast(&d.dark.text, bg) >= 4.5); prop_assert!(contrast(&d.dark.focus, bg) >= 3.0); }
            prop_assert!(contrast(&d.light.strong, "#FFFFFF") >= 4.5);
            prop_assert!(contrast(&d.dark.strong, "#191919") >= 4.5);
        }
    }
}
