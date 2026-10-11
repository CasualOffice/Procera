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
    // Acknowledgement campaigns (BPMS-046). Vars: title, version, due, link (+ name for escalations).
    ("campaign_assigned.title", "Please acknowledge {{ title }}"),
    ("campaign_assigned.body", "Read version {{ version }} and acknowledge it by {{ due }}."),
    ("campaign_assigned.subject", "Please acknowledge {{ title }} by {{ due }}"),
    (
        "campaign_assigned.text",
        "You are asked to read and acknowledge {{ title }} (version {{ version }}) by {{ due }}.\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "campaign_assigned.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">You are asked to read and acknowledge <strong>{{ title }}</strong> (version {{ version }}) by <strong>{{ due }}</strong>.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Read and acknowledge</a></p>
<p style="margin:0;font-size:14px;color:#5f5f5a">If the button doesn't work, paste this address into your browser:<br><span style="word-break:break-all">{{ link }}</span></p>{% endblock %}
{% block footer %}You receive this because you are in the audience of an acknowledgement campaign.{% endblock %}"#,
    ),
    ("campaign_reminder.title", "Reminder: acknowledge {{ title }}"),
    ("campaign_reminder.body", "Version {{ version }} is waiting for your acknowledgement (due {{ due }})."),
    ("campaign_reminder.subject", "Reminder: acknowledge {{ title }} (due {{ due }})"),
    (
        "campaign_reminder.text",
        "{{ title }} (version {{ version }}) is still waiting for your acknowledgement. It is due {{ due }}.\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "campaign_reminder.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ title }}</strong> (version {{ version }}) is still waiting for your acknowledgement. It is due <strong>{{ due }}</strong>.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Read and acknowledge</a></p>{% endblock %}
{% block footer %}You receive this because an acknowledgement assigned to you is open.{% endblock %}"#,
    ),
    ("campaign_escalated.title", "{{ name }} has not acknowledged {{ title }}"),
    ("campaign_escalated.body", "Version {{ version }} was due {{ due }}. You are their manager."),
    ("campaign_escalated.subject", "{{ name }} has not acknowledged {{ title }}"),
    (
        "campaign_escalated.text",
        "{{ name }} has not yet acknowledged {{ title }} (version {{ version }}), due {{ due }}. You receive this as their manager.\n\n{{ link }}\n",
    ),
    (
        "campaign_escalated.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ name }}</strong> has not yet acknowledged <strong>{{ title }}</strong> (version {{ version }}), due {{ due }}.</p>
<p style="margin:0">You receive this as their manager. <a href="{{ link }}">Open in Procera</a></p>{% endblock %}
{% block footer %}Escalation from an acknowledgement campaign.{% endblock %}"#,
    ),
    // BPMS-047 (FR-AN04): announcement acknowledgements run on the campaign engine. Vars: title, due,
    // link (+ name for escalations); `version` is null.
    ("announcement_ack_reminder.title", "Please confirm you read {{ title }}"),
    ("announcement_ack_reminder.body", "This announcement asks for your acknowledgement by {{ due }}."),
    ("announcement_ack_reminder.subject", "Reminder: please confirm you read {{ title }} (due {{ due }})"),
    (
        "announcement_ack_reminder.text",
        "Please confirm you read the announcement {{ title }}. It asks for your acknowledgement by {{ due }}.\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "announcement_ack_reminder.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">Please confirm you read the announcement <strong>{{ title }}</strong>. It asks for your acknowledgement by <strong>{{ due }}</strong>.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Read and acknowledge</a></p>{% endblock %}
{% block footer %}You receive this because an announcement you received asks for acknowledgement.{% endblock %}"#,
    ),
    ("announcement_ack_escalated.title", "{{ name }} has not acknowledged {{ title }}"),
    ("announcement_ack_escalated.body", "The announcement was due {{ due }}. You are their manager."),
    ("announcement_ack_escalated.subject", "{{ name }} has not acknowledged {{ title }}"),
    (
        "announcement_ack_escalated.text",
        "{{ name }} has not yet acknowledged the announcement {{ title }}, due {{ due }}. You receive this as their manager.\n\n{{ link }}\n",
    ),
    (
        "announcement_ack_escalated.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ name }}</strong> has not yet acknowledged the announcement <strong>{{ title }}</strong>, due {{ due }}.</p>
<p style="margin:0">You receive this as their manager. <a href="{{ link }}">Open in Procera</a></p>{% endblock %}
{% block footer %}Escalation from an announcement that asks for acknowledgement.{% endblock %}"#,
    ),
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
    // BPMS-064: a background audit export is ready. Vars: rows.
    ("audit_export_ready.title", "Your audit export is ready"),
    (
        "audit_export_ready.body",
        "{{ rows }} event{% if rows != 1 %}s{% endif %}. Download it from the audit log.",
    ),
    // BPMS-066: an untouched draft will be deleted by the retention job. Vars: title, version, date.
    ("draft_expiring.title", "Draft {{ title }} v{{ version }} will be deleted on {{ date }}"),
    ("draft_expiring.body", "Nobody has edited it for a while. Edit it to keep it, or let it go."),
    // BPMS-048: one email with everything since the last digest. Vars: workspace, count, items[{title, link}].
    ("digest.subject", "Your {{ workspace }} digest: {{ count }} update{% if count != 1 %}s{% endif %}"),
    (
        "digest.text",
        "Here is what happened in {{ workspace }} since your last digest.\n\n{% for i in items %}- {{ i.title }}{% if i.link %}\n  {{ i.link }}{% endif %}\n{% endfor %}\nChange how often you get this in your notification preferences.\n",
    ),
    (
        "digest.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">Here is what happened in <strong>{{ workspace }}</strong> since your last digest.</p>
<ul style="margin:0 0 8px;padding-left:20px">{% for i in items %}<li style="margin:0 0 8px">{% if i.link %}<a href="{{ i.link }}">{{ i.title }}</a>{% else %}{{ i.title }}{% endif %}</li>{% endfor %}</ul>{% endblock %}
{% block footer %}Change how often you get this in your notification preferences.{% endblock %}"#,
    ),
    // BPMS-040: a draft waits for review. Vars: title, version, author, link.
    ("review_requested.title", "Review {{ title }}"),
    ("review_requested.body", "{{ author }} submitted version {{ version }} for review."),
    ("review_requested.subject", "Review requested: {{ title }} v{{ version }}"),
    (
        "review_requested.text",
        "{{ author }} submitted {{ title }} (version {{ version }}) for review.\n\nOpen it to approve or request changes:\n{{ link }}\n",
    ),
    (
        "review_requested.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">{{ author }} submitted <strong>{{ title }}</strong> (version {{ version }}) for review.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Review</a></p>
<p style="margin:0;font-size:14px;color:#5f5f5a">If the button doesn't work, paste this address into your browser:<br><span style="word-break:break-all">{{ link }}</span></p>{% endblock %}
{% block footer %}You receive this because you can review this kind of document.{% endblock %}"#,
    ),
    // BPMS-040: the review outcome, to the author. Vars: title, version, outcome
    // (approved | changes_requested | reject), comment (optional), link.
    (
        "review_decided.title",
        "{% if outcome == 'approved' %}{{ title }} is approved{% elif outcome == 'reject' %}{{ title }} was rejected{% else %}Changes requested on {{ title }}{% endif %}",
    ),
    (
        "review_decided.body",
        "{% if outcome == 'approved' %}Version {{ version }} is ready to publish.{% else %}Version {{ version }} is back in draft{% if comment %}: “{{ comment }}”{% else %}.{% endif %}{% endif %}",
    ),
    (
        "review_decided.subject",
        "{% if outcome == 'approved' %}Approved: {{ title }} v{{ version }}{% elif outcome == 'reject' %}Rejected: {{ title }} v{{ version }}{% else %}Changes requested: {{ title }} v{{ version }}{% endif %}",
    ),
    (
        "review_decided.text",
        "{% if outcome == 'approved' %}{{ title }} (version {{ version }}) is approved and ready to publish.{% else %}{{ title }} (version {{ version }}) is back in draft.{% if comment %}\n\nReviewer's comment: {{ comment }}{% endif %}{% endif %}\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "review_decided.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">{% if outcome == 'approved' %}<strong>{{ title }}</strong> (version {{ version }}) is approved and ready to publish.{% else %}<strong>{{ title }}</strong> (version {{ version }}) is back in draft.{% endif %}</p>
{% if comment and outcome != 'approved' %}<p style="margin:0 0 16px;padding:12px 16px;background:#f4f4f2;border-radius:6px">{{ comment }}</p>{% endif %}
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open the document</a></p>{% endblock %}
{% block footer %}You receive this because you wrote this version.{% endblock %}"#,
    ),
    // BPMS-047: an announcement waits for approval. Vars: title, author, link.
    ("announcement_approval.title", "Approve “{{ title }}”"),
    ("announcement_approval.body", "{{ author }} sent an announcement for approval."),
    ("announcement_approval.subject", "Approval requested: {{ title }}"),
    (
        "announcement_approval.text",
        "{{ author }} sent the announcement “{{ title }}” for approval.\n\nOpen it to approve or send it back:\n{{ link }}\n",
    ),
    (
        "announcement_approval.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">{{ author }} sent the announcement <strong>{{ title }}</strong> for approval.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Review the announcement</a></p>{% endblock %}
{% block footer %}You receive this because you can publish announcements.{% endblock %}"#,
    ),
    // BPMS-047: the approval outcome, to the author. Vars: title, outcome (approved | rejected), comment, link.
    (
        "announcement_decided.title",
        "{% if outcome == 'approved' %}“{{ title }}” is approved{% else %}“{{ title }}” was sent back{% endif %}",
    ),
    (
        "announcement_decided.body",
        "{% if outcome == 'approved' %}It is published, or will be at its scheduled time.{% else %}It is back in draft{% if comment %}: “{{ comment }}”{% else %}.{% endif %}{% endif %}",
    ),
    (
        "announcement_decided.subject",
        "{% if outcome == 'approved' %}Approved: {{ title }}{% else %}Sent back: {{ title }}{% endif %}",
    ),
    (
        "announcement_decided.text",
        "{% if outcome == 'approved' %}Your announcement “{{ title }}” is approved.{% else %}Your announcement “{{ title }}” is back in draft.{% if comment %}\n\nComment: {{ comment }}{% endif %}{% endif %}\n\n{{ link }}\n",
    ),
    (
        "announcement_decided.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">{% if outcome == 'approved' %}Your announcement <strong>{{ title }}</strong> is approved.{% else %}Your announcement <strong>{{ title }}</strong> is back in draft.{% endif %}</p>
{% if comment and outcome != 'approved' %}<p style="margin:0 0 16px;padding:12px 16px;background:#f4f4f2;border-radius:6px">{{ comment }}</p>{% endif %}
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open it</a></p>{% endblock %}
{% block footer %}You receive this because you wrote this announcement.{% endblock %}"#,
    ),
    // BPMS-044: an artifact's review date passed. Vars: title.
    ("review_overdue.title", "{{ title }} is due for review"),
    (
        "review_overdue.body",
        "Its review date has passed. Confirm it is still correct, or start a new version.",
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
    fn review_templates_render_every_outcome() {
        let ctx =
            json!({"title": "Travel <policy>", "version": 2, "author": "Ann", "link": "https://x.test/l"});
        let r = email("review_requested", &ctx).expect("renders");
        assert!(r.subject.contains("Travel <policy> v2") && r.html.contains("Travel &lt;policy&gt;"));
        assert!(in_app("review_requested", &ctx).expect("renders").body.contains("Ann submitted version 2"));
        for (outcome, comment, word) in [
            ("approved", None, "ready to publish"),
            ("changes_requested", Some("Fix <b>"), "back in draft"),
            ("reject", Some("No"), "back in draft"),
        ] {
            let ctx = json!({"title": "T", "version": 1, "outcome": outcome, "comment": comment, "link": "https://x.test/l"});
            let r = email("review_decided", &ctx).expect("renders");
            assert!(r.text.contains(word), "{outcome}");
            assert!(!r.html.contains("<b>"), "{outcome}: comment is escaped");
            assert!(!in_app("review_decided", &ctx).expect("renders").title.is_empty());
        }
    }

    #[test]
    fn announcement_approval_templates_render() {
        let ctx = json!({"title": "Fire <drill>", "author": "Ann", "link": "https://x.test/l"});
        assert!(email("announcement_approval", &ctx).expect("renders").html.contains("Fire &lt;drill&gt;"));
        for outcome in ["approved", "rejected"] {
            let ctx = json!({"title": "T", "outcome": outcome, "comment": "Too <long>", "link": "https://x.test/l"});
            let r = email("announcement_decided", &ctx).expect("renders");
            assert!(!r.html.contains("<long>") && r.text.contains("https://x.test/l"), "{outcome}");
            assert!(!in_app("announcement_decided", &ctx).expect("renders").title.is_empty());
        }
    }

    #[test]
    fn campaign_templates_render_in_app_and_email() {
        let ctx = json!({"title": "Code <of> conduct", "version": 3, "due": "25 Oct 2026", "link": "https://x.test/a", "name": "Ben"});
        for kind in ["campaign_assigned", "campaign_reminder", "campaign_escalated"] {
            let r = email(kind, &ctx).expect("renders");
            assert!(r.subject.contains("Code <of> conduct"), "{kind}");
            assert!(
                r.html.contains("Code &lt;of&gt; conduct") && r.text.contains("https://x.test/a"),
                "{kind}"
            );
            assert!(!in_app(kind, &ctx).expect("renders").title.is_empty());
        }
    }

    #[test]
    fn announcement_ack_templates_render_without_a_version() {
        let ctx = json!({"title": "Office <move>", "version": null, "due": "25 Oct 2026", "link": "https://x.test/announcements/1", "name": "Ben"});
        for kind in ["announcement_ack_reminder", "announcement_ack_escalated"] {
            let r = email(kind, &ctx).expect("renders");
            assert!(r.subject.contains("Office <move>") && !r.text.contains("version"), "{kind}");
            assert!(r.html.contains("Office &lt;move&gt;") && r.text.contains("/announcements/1"), "{kind}");
            assert!(!in_app(kind, &ctx).expect("renders").title.is_empty());
        }
        assert_eq!(
            in_app("announcement_ack_reminder", &ctx).expect("renders").title,
            "Please confirm you read Office <move>"
        );
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
    fn digest_lists_items_and_escapes_titles() {
        let ctx = json!({"workspace": "Acme", "count": 2, "items": [{"title": "Office <move>", "link": "https://x.test/a"}, {"title": "Badge rules", "link": null}]});
        let r = email("digest", &ctx).expect("renders");
        assert_eq!(r.subject, "Your Acme digest: 2 updates");
        assert!(r.text.contains("- Office <move>\n  https://x.test/a") && r.text.contains("- Badge rules"));
        assert!(r.html.contains("Office &lt;move&gt;"));
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
