//! Notification templates (FR-N02). Templates live in code, render in a sandboxed engine with strict
//! undefined variables, and HTML bodies are auto-escaped. `en` is the only locale today.
//!
//! Workspace terminology (CFG-008): every template sees `terms.<term>.singular`, `.plural`, `.lower`
//! and `.lower_plural` for `request`, `process`, `step`, `sop`, `case` and `work`. `Notify` passes
//! the workspace's labels; without them the product defaults apply.

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
    // Account recovery (docs/06 §3.6.1): system emails, not tied to a workspace. Vars: name, link, minutes.
    ("password_reset.subject", "Reset your Procera password"),
    (
        "password_reset.text",
        "Hi {{ name }},\n\nSomeone asked to reset the password of your Procera account. To choose a new one, open this link:\n{{ link }}\n\nThe link works once and expires in {{ minutes }} minutes. Two-step verification stays on, so you'll still confirm your next sign-in.\n\nIf you didn't ask for this, ignore this email: your password stays the same.\n",
    ),
    (
        "password_reset.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">Hi {{ name }},</p>
<p style="margin:0 0 16px">Someone asked to reset the password of your Procera account. To choose a new one, use the button below.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Choose a new password</a></p>
<p style="margin:0 0 16px;font-size:14px;color:#5f5f5a">The link works once and expires in {{ minutes }} minutes. Two-step verification stays on, so you'll still confirm your next sign-in. If the button doesn't work, paste this address into your browser:<br><span style="word-break:break-all">{{ link }}</span></p>{% endblock %}
{% block footer %}If you didn't ask for this, ignore this email: your password stays the same.{% endblock %}"#,
    ),
    // Vars: name, how ("reset" with a link | "changed" while signed in or at a required change).
    ("password_changed.subject", "Your Procera password was changed"),
    (
        "password_changed.text",
        "Hi {{ name }},\n\nThe password of your Procera account was {% if how == 'reset' %}reset with an email link{% else %}changed{% endif %}. {% if how == 'reset' %}Every session was signed out.{% else %}Your other sessions were signed out.{% endif %}\n\nIf this wasn't you, reset your password now from the sign-in page and tell your administrator.\n",
    ),
    (
        "password_changed.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">Hi {{ name }},</p>
<p style="margin:0 0 16px">The password of your Procera account was {% if how == 'reset' %}reset with an email link{% else %}changed{% endif %}. {% if how == 'reset' %}Every session was signed out.{% else %}Your other sessions were signed out.{% endif %}</p>
<p style="margin:0;font-size:14px;color:#5f5f5a">If this wasn't you, reset your password now from the sign-in page and tell your administrator.</p>{% endblock %}
{% block footer %}This is a security notice about your account.{% endblock %}"#,
    ),
    // An administrator changed the account's security. Vars: name, workspace, action
    // (require_password_change | sign_out_everywhere | reset_mfa).
    ("account_security_changed.subject", "An administrator changed your Procera account security"),
    (
        "account_security_changed.text",
        "Hi {{ name }},\n\nAn administrator of {{ workspace }} {% if action == 'require_password_change' %}asked you to choose a new password the next time you sign in{% elif action == 'sign_out_everywhere' %}signed you out of Procera on every device{% else %}reset your two-step verification; you'll set it up again when you next sign in{% endif %}.\n\nIf you didn't expect this, contact your administrator.\n",
    ),
    (
        "account_security_changed.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">Hi {{ name }},</p>
<p style="margin:0 0 16px">An administrator of <strong>{{ workspace }}</strong> {% if action == 'require_password_change' %}asked you to choose a new password the next time you sign in{% elif action == 'sign_out_everywhere' %}signed you out of Procera on every device{% else %}reset your two-step verification; you'll set it up again when you next sign in{% endif %}.</p>
<p style="margin:0;font-size:14px;color:#5f5f5a">If you didn't expect this, contact your administrator.</p>{% endblock %}
{% block footer %}This is a security notice about your account.{% endblock %}"#,
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
    // BPMS-040: a draft waits for review. Vars: title, version, author, kind (sop | policy), link.
    ("review_requested.title", "Review {{ title }}"),
    (
        "review_requested.body",
        "{{ author }} submitted version {{ version }} of this {{ terms.sop.lower if kind == 'sop' else 'policy' }} for review.",
    ),
    ("review_requested.subject", "Review requested: {{ title }} v{{ version }}"),
    (
        "review_requested.text",
        "{{ author }} submitted the {{ terms.sop.lower if kind == 'sop' else 'policy' }} {{ title }} (version {{ version }}) for review.\n\nOpen it to approve or request changes:\n{{ link }}\n",
    ),
    (
        "review_requested.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">{{ author }} submitted the {{ terms.sop.lower if kind == 'sop' else 'policy' }} <strong>{{ title }}</strong> (version {{ version }}) for review.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Review</a></p>
<p style="margin:0;font-size:14px;color:#5f5f5a">If the button doesn't work, paste this address into your browser:<br><span style="word-break:break-all">{{ link }}</span></p>{% endblock %}
{% block footer %}You receive this because you can review {{ terms.sop.lower_plural if kind == 'sop' else 'policies' }}.{% endblock %}"#,
    ),
    // BPMS-040: the review outcome, to the author. Vars: title, version, kind (sop | policy), outcome
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
        "{% set noun = terms.sop.singular if kind == 'sop' else 'Policy' %}{% if outcome == 'approved' %}{{ noun }} {{ title }} (version {{ version }}) is approved and ready to publish.{% else %}{{ noun }} {{ title }} (version {{ version }}) is back in draft.{% if comment %}\n\nReviewer's comment: {{ comment }}{% endif %}{% endif %}\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "review_decided.html",
        r#"{% extends "layout.html" %}{% block content %}{% set noun = terms.sop.singular if kind == 'sop' else 'Policy' %}<p style="margin:0 0 16px">{% if outcome == 'approved' %}{{ noun }} <strong>{{ title }}</strong> (version {{ version }}) is approved and ready to publish.{% else %}{{ noun }} <strong>{{ title }}</strong> (version {{ version }}) is back in draft.{% endif %}</p>
{% if comment and outcome != 'approved' %}<p style="margin:0 0 16px;padding:12px 16px;background:#f4f4f2;border-radius:6px">{{ comment }}</p>{% endif %}
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open the {{ terms.sop.lower if kind == 'sop' else 'policy' }}</a></p>{% endblock %}
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
    // Requests and approvals (BPMS-085). Common vars: display_id, title, type_name, link.
    // work_assigned adds step, initiator, due (or none).
    ("work_assigned.title", "{{ step }}: {{ display_id }}"),
    ("work_assigned.body", "{{ initiator }} asked for {{ title }}.{% if due %} Due {{ due }}.{% endif %}"),
    ("work_assigned.subject", "{{ step }} needed: {{ display_id }} {{ title }}"),
    (
        "work_assigned.text",
        "{{ initiator }} submitted a {{ type_name | lower }} ({{ display_id }}): {{ title }}.\n\nYou are asked for: {{ step }}{% if due %}, by {{ due }}{% endif %}.\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "work_assigned.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ initiator }}</strong> submitted a {{ type_name | lower }} (<strong>{{ display_id }}</strong>): {{ title }}.</p>
<p style="margin:0 0 16px">You are asked for: <strong>{{ step }}</strong>{% if due %}, by <strong>{{ due }}</strong>{% endif %}.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open it</a></p>{% endblock %}
{% block footer %}You receive this because a step of this {{ terms.request.lower }} is offered to you.{% endblock %}"#,
    ),
    // work_reminder: same vars as work_assigned (BPMS-085 reminders).
    (
        "work_reminder.title",
        "Reminder: {{ display_id }} is waiting for your decision{% if due %}, due {{ due }}{% endif %}",
    ),
    ("work_reminder.body", "{{ step }} for {{ title }}, from {{ initiator }}."),
    ("work_reminder.subject", "Reminder: {{ display_id }} {{ title }} is waiting for your decision"),
    (
        "work_reminder.text",
        "Reminder: {{ type_name }} {{ display_id }} ({{ title }}) from {{ initiator }} is waiting for your decision: {{ step }}{% if due %}, due {{ due }}{% endif %}.\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "work_reminder.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">Reminder: {{ type_name | lower }} <strong>{{ display_id }}</strong> ({{ title }}) from {{ initiator }} is waiting for your decision.</p>
<p style="margin:0 0 16px">{{ step }}{% if due %}, due <strong>{{ due }}</strong>{% endif %}.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open it</a></p>{% endblock %}
{% block footer %}You receive this because a step of this {{ terms.request.lower }} is offered to you.{% endblock %}"#,
    ),
    // work_escalated: work_assigned vars plus reason (or none). To whoever an escalation names.
    ("work_escalated.title", "Overdue: {{ step }} for {{ display_id }}"),
    (
        "work_escalated.body",
        "{{ initiator }}’s {{ title }} is still waiting{% if due %} (due {{ due }}){% endif %}.",
    ),
    ("work_escalated.subject", "Overdue: {{ step }} for {{ display_id }} {{ title }}"),
    (
        "work_escalated.text",
        "{{ step }} for {{ type_name | lower }} {{ display_id }} ({{ title }}) from {{ initiator }} is overdue{% if due %}: it was due {{ due }}{% endif %}.\nYou are told because the process escalates overdue steps to you.\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "work_escalated.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ step }}</strong> for {{ type_name | lower }} <strong>{{ display_id }}</strong> ({{ title }}) from {{ initiator }} is overdue{% if due %}: it was due <strong>{{ due }}</strong>{% endif %}.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open it</a></p>{% endblock %}
{% block footer %}You receive this because this process escalates overdue steps to you.{% endblock %}"#,
    ),
    // work_reassigned: work_assigned vars plus to (names) and reason. To the people taken off a step.
    ("work_reassigned.title", "{{ display_id }} moved to {{ to }}"),
    ("work_reassigned.body", "{{ step }} for {{ title }} is no longer waiting for you: {{ reason }}."),
    ("work_reassigned.subject", "{{ display_id }} {{ title }}: {{ step }} moved to {{ to }}"),
    (
        "work_reassigned.text",
        "{{ step }} for {{ type_name | lower }} {{ display_id }} ({{ title }}) is now with {{ to }} ({{ reason }}). You don’t need to do anything.\n\n{{ link }}\n",
    ),
    (
        "work_reassigned.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ step }}</strong> for {{ type_name | lower }} <strong>{{ display_id }}</strong> ({{ title }}) is now with <strong>{{ to }}</strong> ({{ reason }}). You don’t need to do anything.</p>{% endblock %}
{% block footer %}You receive this because this step was offered to you.{% endblock %}"#,
    ),
    // request_decided adds step, outcome (approved | rejected | changes_requested), decided_by, comment (or none).
    (
        "request_decided.title",
        "{{ display_id }}: {% if outcome == 'approved' %}{{ step }} approved{% elif outcome == 'changes_requested' %}changes requested{% else %}{{ step }} rejected{% endif %}",
    ),
    (
        "request_decided.body",
        "{{ decided_by }} {% if outcome == 'approved' %}approved{% elif outcome == 'changes_requested' %}sent it back to you{% else %}rejected it{% endif %}.{% if comment %} “{{ comment }}”{% endif %}",
    ),
    (
        "request_decided.subject",
        "{{ display_id }} {{ title }}: {% if outcome == 'approved' %}{{ step }} approved{% elif outcome == 'changes_requested' %}changes requested{% else %}{{ step }} rejected{% endif %}",
    ),
    (
        "request_decided.text",
        "{{ decided_by }} {% if outcome == 'approved' %}approved {{ step }} for{% elif outcome == 'changes_requested' %}sent back{% else %}rejected{% endif %} your {{ type_name | lower }} {{ display_id }} ({{ title }}).\n{% if comment %}\n{{ comment }}\n{% endif %}\nOpen it:\n{{ link }}\n",
    ),
    (
        "request_decided.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ decided_by }}</strong> {% if outcome == 'approved' %}approved {{ step }} for{% elif outcome == 'changes_requested' %}sent back{% else %}rejected{% endif %} your {{ type_name | lower }} <strong>{{ display_id }}</strong> ({{ title }}).</p>
{% if comment %}<p style="margin:0 0 16px;padding:12px 16px;background:#f4f4f2;border-radius:6px">{{ comment }}</p>{% endif %}
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open it</a></p>{% endblock %}
{% block footer %}You receive this because you submitted this {{ terms.request.lower }}.{% endblock %}"#,
    ),
    // request_completed adds status (approved | rejected).
    ("request_completed.title", "{{ display_id }} {{ status }}"),
    ("request_completed.body", "Your {{ type_name | lower }} {{ title }} was {{ status }}."),
    ("request_completed.subject", "{{ display_id }} {{ title }} was {{ status }}"),
    (
        "request_completed.text",
        "Your {{ type_name | lower }} {{ display_id }} ({{ title }}) was {{ status }}.\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "request_completed.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px">Your {{ type_name | lower }} <strong>{{ display_id }}</strong> ({{ title }}) was <strong>{{ status }}</strong>.</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open it</a></p>{% endblock %}
{% block footer %}You receive this because you submitted this {{ terms.request.lower }}.{% endblock %}"#,
    ),
    // request_notice: a notify step of a process. Adds step (the notify step's name).
    ("request_notice.title", "{{ step }}: {{ display_id }}"),
    ("request_notice.body", "{{ title }}"),
    ("request_notice.subject", "{{ step }}: {{ display_id }} {{ title }}"),
    (
        "request_notice.text",
        "{{ step }}: {{ type_name }} {{ display_id }} ({{ title }}).\n\nOpen it:\n{{ link }}\n",
    ),
    (
        "request_notice.html",
        r#"{% extends "layout.html" %}{% block content %}<p style="margin:0 0 16px"><strong>{{ step }}</strong>: {{ type_name }} <strong>{{ display_id }}</strong> ({{ title }}).</p>
<p style="margin:0 0 24px"><a href="{{ link }}" style="display:inline-block;background:#1c1c1a;color:#ffffff;text-decoration:none;padding:12px 20px;border-radius:6px;font-weight:600">Open it</a></p>{% endblock %}
{% block footer %}You receive this because this {{ terms.request.lower }} concerns you.{% endblock %}"#,
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

/// `ctx` with `terms` for `stored` (the workspace's `terminology.overrides` value) unless the
/// caller already set them. Non-object contexts are returned as they are.
pub fn with_terms(ctx: &Value, stored: &Value) -> Value {
    let mut c = ctx.clone();
    if let Some(o) = c.as_object_mut()
        && !o.contains_key("terms")
    {
        o.insert("terms".into(), procera_settings::terms::template_context(stored));
    }
    c
}

/// Renders an email. Without `terms` in `ctx`, the product's default terminology applies.
pub fn email(kind: &str, ctx: &Value) -> Result<Rendered, NotifyError> {
    let env = env()?;
    let ctx = &with_terms(ctx, &Value::Null);
    Ok(Rendered {
        subject: render(&env, &format!("{kind}.subject"), ctx)?.replace(['\r', '\n'], " "),
        text: render(&env, &format!("{kind}.text"), ctx)?,
        html: render(&env, &format!("{kind}.html"), ctx)?,
    })
}

/// Renders an in-app notification. Without `terms` in `ctx`, the default terminology applies.
pub fn in_app(kind: &str, ctx: &Value) -> Result<RenderedInApp, NotifyError> {
    let env = env()?;
    let ctx = &with_terms(ctx, &Value::Null);
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
    fn request_templates_render() {
        let base = json!({"display_id": "LR-2026-000001", "title": "Annual <leave>", "type_name": "Leave request", "link": "https://x.test/r"});
        let mut w = base.clone();
        w["step"] = json!("Manager approval");
        w["initiator"] = json!("Ann");
        w["due"] = json!(null);
        let r = email("work_assigned", &w).expect("renders");
        assert!(r.text.contains("Ann submitted a leave request") && !r.text.contains("by "));
        assert!(r.html.contains("Annual &lt;leave&gt;"));
        assert!(in_app("work_assigned", &w).expect("renders").title.starts_with("Manager approval"));
        w["due"] = json!("14 Oct 2026");
        assert_eq!(
            in_app("work_reminder", &w).expect("renders").title,
            "Reminder: LR-2026-000001 is waiting for your decision, due 14 Oct 2026"
        );
        assert!(email("work_reminder", &w).expect("renders").text.contains("due 14 Oct 2026"));
        w["reason"] = json!("overdue");
        assert!(
            in_app("work_escalated", &w).expect("renders").title.starts_with("Overdue: Manager approval")
        );
        email("work_escalated", &w).expect("renders");
        w["to"] = json!("Cleo");
        w["reason"] = json!("escalated after due");
        assert!(
            email("work_reassigned", &w)
                .expect("renders")
                .text
                .contains("now with Cleo (escalated after due)")
        );
        in_app("work_reassigned", &w).expect("renders");
        for (outcome, comment) in
            [("approved", None), ("rejected", Some("No budget")), ("changes_requested", Some("Dates"))]
        {
            let mut d = base.clone();
            d["step"] = json!("Manager approval");
            d["outcome"] = json!(outcome);
            d["decided_by"] = json!("Bob");
            d["comment"] = json!(comment);
            let r = email("request_decided", &d).expect("renders");
            assert!(r.text.contains("Bob"), "{outcome}");
            assert_eq!(r.text.contains("No budget"), comment == Some("No budget"));
            in_app("request_decided", &d).expect("renders");
        }
        let mut c = base.clone();
        c["status"] = json!("approved");
        assert!(email("request_completed", &c).expect("renders").subject.ends_with("was approved"));
        let mut n = base;
        n["step"] = json!("Access granted");
        assert!(in_app("request_notice", &n).expect("renders").title.starts_with("Access granted"));
    }

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
        let ctx = json!({"title": "Travel <policy>", "version": 2, "author": "Ann", "kind": "policy", "link": "https://x.test/l"});
        let r = email("review_requested", &ctx).expect("renders");
        assert!(r.subject.contains("Travel <policy> v2") && r.html.contains("Travel &lt;policy&gt;"));
        assert!(in_app("review_requested", &ctx).expect("renders").body.contains("Ann submitted version 2"));
        for (outcome, comment, word) in [
            ("approved", None, "ready to publish"),
            ("changes_requested", Some("Fix <b>"), "back in draft"),
            ("reject", Some("No"), "back in draft"),
        ] {
            let ctx = json!({"title": "T", "version": 1, "kind": "sop", "outcome": outcome, "comment": comment, "link": "https://x.test/l"});
            let r = email("review_decided", &ctx).expect("renders");
            assert!(r.text.starts_with("SOP T"), "{outcome}: default term");
            assert!(r.text.contains(word), "{outcome}");
            assert!(!r.html.contains("<b>"), "{outcome}: comment is escaped");
            assert!(!in_app("review_decided", &ctx).expect("renders").title.is_empty());
        }
    }

    #[test]
    fn workspace_terms_reach_review_emails() {
        let stored = json!({"sop": {"singular": "Work Instruction", "plural": "Work Instructions"}});
        let ctx = with_terms(
            &json!({"title": "Lockout <tagout>", "version": 3, "author": "Ann", "kind": "sop", "link": "https://x.test/l"}),
            &stored,
        );
        let r = email("review_requested", &ctx).expect("renders");
        assert!(r.text.starts_with("Ann submitted the work instruction Lockout <tagout>"), "{}", r.text);
        assert!(r.html.contains("the work instruction <strong>Lockout &lt;tagout&gt;</strong>"));
        assert!(r.html.contains("you can review work instructions."));
        assert!(!r.text.contains("SOP") && !r.html.contains("SOP"));
        let n = in_app("review_requested", &ctx).expect("renders");
        assert_eq!(n.body, "Ann submitted version 3 of this work instruction for review.");
        let mut decided = ctx.clone();
        decided["outcome"] = json!("approved");
        decided["comment"] = json!(null);
        let r = email("review_decided", &decided).expect("renders");
        assert!(
            r.text.starts_with("Work Instruction Lockout <tagout> (version 3) is approved"),
            "{}",
            r.text
        );
        assert!(r.html.contains("Open the work instruction"));
        // A policy never takes the SOP term.
        let mut policy = ctx.clone();
        policy["kind"] = json!("policy");
        assert!(email("review_requested", &policy).expect("renders").text.contains("the policy Lockout"));
    }

    #[test]
    fn request_terms_are_ready_for_request_templates() {
        // No request emails exist yet (they arrive with requests); their templates will read
        // `terms.request.*` from the same context.
        let mut env = env().expect("env");
        env.add_template("t", "{{ terms.request.singular }} {{ id }}: your {{ terms.request.lower }} is open ({{ terms.request.lower_plural }})")
            .expect("template");
        let ctx = with_terms(
            &json!({"id": "TCK-1"}),
            &json!({"request": {"singular": "Ticket", "plural": "Tickets"}}),
        );
        assert_eq!(render(&env, "t", &ctx).expect("renders"), "Ticket TCK-1: your ticket is open (tickets)");
        let ctx = with_terms(&json!({"id": "REQ-1"}), &json!({}));
        assert_eq!(
            render(&env, "t", &ctx).expect("renders"),
            "Request REQ-1: your request is open (requests)"
        );
        // A caller's own terms win; a template naming an unknown term still fails (strict).
        let ctx = with_terms(
            &json!({"id": "X", "terms": {"request": {"singular": "Job", "lower": "job", "lower_plural": "jobs"}}}),
            &json!({}),
        );
        assert_eq!(render(&env, "t", &ctx).expect("renders"), "Job X: your job is open (jobs)");
        env.add_template("u", "{{ terms.invoice.singular }}").expect("template");
        assert!(render(&env, "u", &with_terms(&json!({}), &json!({}))).is_err());
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
    fn account_recovery_templates_render_and_escape() {
        let r = email(
            "password_reset",
            &json!({"name": "Ann <b>", "link": "https://x.test/reset-password?token=abc", "minutes": 30}),
        )
        .expect("renders");
        assert_eq!(r.subject, "Reset your Procera password");
        assert!(r.text.contains("https://x.test/reset-password?token=abc") && r.text.contains("30 minutes"));
        assert!(r.html.contains("Ann &lt;b&gt;") && !r.html.contains("<b>"));
        for (how, words) in [("reset", "Every session"), ("changed", "other sessions")] {
            let r = email("password_changed", &json!({"name": "Ann", "how": how})).expect("renders");
            assert!(r.text.contains(words) && r.html.contains(words), "{how}");
        }
        for (action, words) in [
            ("require_password_change", "choose a new password"),
            ("sign_out_everywhere", "every device"),
            ("reset_mfa", "two-step verification"),
        ] {
            let r = email(
                "account_security_changed",
                &json!({"name": "Ann", "workspace": "Acme & Co", "action": action}),
            )
            .expect("renders");
            assert!(r.text.contains(words) && r.html.contains("Acme &amp; Co"), "{action}");
        }
        assert!(email("password_reset", &json!({"name": "Ann"})).is_err(), "the link is required");
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
