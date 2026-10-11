//! Notification templates (FR-N02). Templates live in code, render in a sandboxed engine with strict
//! undefined variables, and HTML bodies are auto-escaped. Localisation and workspace terminology
//! overrides (CFG-008) plug in here later; `en` is the only locale today.

use minijinja::{Environment, UndefinedBehavior};
use serde_json::Value;

use crate::NotifyError;

/// A rendered email.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub subject: String,
    pub text: String,
    pub html: String,
}

/// A rendered in-app notification.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedInApp {
    pub title: String,
    pub body: String,
}

const LAYOUT: &str = r#"<!doctype html><html lang="en"><body style="margin:0;background:#f6f6f4;font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;color:#1c1c1a">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr><td align="center" style="padding:32px 16px">
<table role="presentation" width="560" cellpadding="0" cellspacing="0" style="max-width:560px;background:#ffffff;border:1px solid #e4e4e0;border-radius:8px">
<tr><td style="padding:28px 32px 8px;font-size:15px;font-weight:600;letter-spacing:.2px">Procera</td></tr>
<tr><td style="padding:8px 32px 28px;font-size:16px;line-height:1.55">{% block content %}{% endblock %}</td></tr>
</table>
<p style="font-size:13px;color:#5f5f5a;margin:16px 0 0">{% block footer %}{% endblock %}</p>
</td></tr></table></body></html>"#;

/// (name, source). `.html` templates are auto-escaped.
const TEMPLATES: &[(&str, &str)] = &[
    ("layout.html", LAYOUT),
    ("invitation.subject", "{{ inviter or 'Someone' }} invited you to {{ workspace }} on Procera"),
    (
        "invitation.text",
        "{{ inviter or 'Someone' }} invited you to join {{ workspace }} on Procera.\n\nAccept the invitation:\n{{ link }}\n\nThe link works once and expires on {{ expires }}. If you weren't expecting this, you can ignore this email.\n",
    ),
    (
        "invitation.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ inviter or "Someone" }}</strong> invited you to join <strong>{{ workspace }}</strong> on Procera.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Accept invitation</a></p>
<p style="margin:0;font-size:14px;color:#5f5f5a">The link works once and expires on {{ expires }}. If the button doesn't work, paste this address into your browser:<br><span style="word-break:break-all">{{ link }}</span></p>{% endblock %}
{% block footer %}If you weren't expecting this, you can ignore this email.{% endblock %}"#,
    ),
    ("member_joined.title", "{{ name }} joined {{ workspace }}"),
    ("member_joined.body", "{{ name }} accepted your invitation."),
    // BPMS-047: a published announcement reaches its recipients.
    ("announcement.title", "{% if critical %}Critical: {% endif %}{{ title }}"),
    (
        "announcement.body",
        "{{ author }} posted an announcement{% if require_ack %} that asks for your acknowledgement{% endif %}.",
    ),
    ("announcement.subject", "{% if critical %}[Critical] {% endif %}{{ title }}"),
    (
        "announcement.text",
        "{{ author }} posted an announcement in {{ workspace }}:\n\n{{ title }}\n\n{% if require_ack %}Please read it and acknowledge it:{% else %}Read it:{% endif %}\n{{ link }}\n",
    ),
    (
        "announcement.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 8px;font-size:14px;color:#5f5f5a">{{ author }} posted an announcement in {{ workspace }}{% if critical %} · <strong style="color:#1c1c1a">Critical</strong>{% endif %}</p>
<p style="margin:0 0 24px;font-size:20px;font-weight:600">{{ title }}</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">{% if require_ack %}Read and acknowledge{% else %}Read announcement{% endif %}</a></p>
<p style="margin:0;font-size:14px;color:#5f5f5a">If the button doesn't work, paste this address into your browser:<br><span style="word-break:break-all">{{ link }}</span></p>{% endblock %}
{% block footer %}You received this because you are in the announcement's audience.{% endblock %}"#,
    ),
];

fn env() -> Result<Environment<'static>, NotifyError> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    for (name, src) in TEMPLATES {
        env.add_template(name, src).map_err(|e| NotifyError::Template(e.to_string()))?;
    }
    Ok(env)
}

fn render(env: &Environment<'_>, name: &str, ctx: &Value) -> Result<String, NotifyError> {
    env.get_template(name)
        .and_then(|t| t.render(ctx))
        .map_err(|e| NotifyError::Template(format!("{name}: {e}")))
}

pub fn email(kind: &str, ctx: &Value) -> Result<Rendered, NotifyError> {
    let env = env()?;
    Ok(Rendered {
        subject: render(&env, &format!("{kind}.subject"), ctx)?.replace(['\r', '\n'], " "),
        text: render(&env, &format!("{kind}.text"), ctx)?,
        html: render(&env, &format!("{kind}.html"), ctx)?,
    })
}

pub fn in_app(kind: &str, ctx: &Value) -> Result<RenderedInApp, NotifyError> {
    let env = env()?;
    Ok(RenderedInApp {
        title: render(&env, &format!("{kind}.title"), ctx)?,
        body: render(&env, &format!("{kind}.body"), ctx)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invitation_renders_and_escapes_html() {
        let ctx = json!({"inviter": "Ann <script>", "workspace": "Acme & Co", "link": "https://x.test/invite?token=abc", "expires": "18 Oct 2026"});
        let r = email("invitation", &ctx).expect("renders");
        assert_eq!(r.subject, "Ann <script> invited you to Acme & Co on Procera");
        assert!(r.text.contains("https://x.test/invite?token=abc"));
        assert!(r.html.contains("Ann &lt;script&gt;") && r.html.contains("Acme &amp; Co"));
        assert!(!r.html.contains("<script>"));
    }

    #[test]
    fn announcement_renders_in_app_and_email() {
        let ctx = json!({"title": "Office <move>", "author": "Ann", "workspace": "Acme", "critical": true, "require_ack": true, "link": "https://x.test/announcements/1"});
        let n = in_app("announcement", &ctx).expect("renders");
        assert_eq!(n.title, "Critical: Office <move>");
        assert!(n.body.contains("acknowledgement"));
        let e = email("announcement", &ctx).expect("renders");
        assert_eq!(e.subject, "[Critical] Office <move>");
        assert!(e.html.contains("Office &lt;move&gt;") && e.html.contains("Read and acknowledge"));
        assert!(e.text.contains("https://x.test/announcements/1"));
    }

    #[test]
    fn missing_variables_fail_instead_of_rendering_blank() {
        assert!(email("invitation", &json!({"workspace": "Acme"})).is_err());
        assert!(email("no_such_kind", &json!({})).is_err());
        assert_eq!(
            in_app("member_joined", &json!({"name": "Ben", "workspace": "Acme"})).expect("renders").title,
            "Ben joined Acme"
        );
    }
}
