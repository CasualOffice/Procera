//! Reviewed starter content for the built-in template packs (BPMS-106, A-38). Written for Procera
//! from general practice, not copied from any published standard or company. Each document opens
//! with a notice that it is a template to adapt and is not legal advice. Placeholders to replace
//! are in square brackets.

use serde_json::{Value, json};

fn p(t: &str) -> Value {
    json!({"type": "paragraph", "text": t})
}
fn h(t: &str) -> Value {
    json!({"type": "heading", "level": 2, "text": t})
}
fn ul(items: &[&str]) -> Value {
    json!({"type": "list", "items": items})
}
fn ol(items: &[&str]) -> Value {
    json!({"type": "list", "ordered": true, "items": items})
}
fn doc(blocks: Vec<Value>) -> Value {
    let mut all = vec![json!({
        "type": "callout",
        "tone": "warning",
        "text": "Template to adapt. This is a starting point from Procera's template library. Replace the parts in square brackets, change anything that does not fit how your organisation works, and take professional advice where the law in your country applies. It is not legal advice. Delete this note before you publish."
    })];
    all.extend(blocks);
    json!({ "blocks": all })
}

pub(super) fn handbook() -> Value {
    doc(vec![
        p(
            "Welcome to [Company]. This handbook explains what we expect from each other and where to find the policies that apply to everyone who works here.",
        ),
        h("How to use this handbook"),
        ul(&[
            "Read it in your first week, then acknowledge it in Procera.",
            "The policies listed below give the detail. When the handbook and a policy differ, the policy applies.",
            "We update the handbook when things change. You will be asked to acknowledge important changes.",
        ]),
        h("Policies that apply to everyone"),
        json!({"type": "table", "header": true, "rows": [
            ["Policy", "What it covers"],
            ["Code of conduct", "How we treat each other, conflicts of interest, raising concerns"],
            ["Acceptable use of IT", "Company devices, accounts, passwords and data"],
            ["Leave and time off", "Holidays, sick leave and how to ask for time off"],
            ["Workplace safety", "Staying safe at work and reporting incidents"],
        ]}),
        h("Questions and concerns"),
        p(
            "Ask your manager first. If that is not possible, contact [People or HR contact]. You can raise a concern without fear of retaliation.",
        ),
        h("Acknowledgement"),
        p(
            "By acknowledging this handbook you confirm that you have read it and the policies it lists, and that you will follow them.",
        ),
    ])
}

pub(super) fn code_of_conduct() -> Value {
    doc(vec![
        h("Purpose"),
        p(
            "This code sets out how we behave at work and when we represent [Company]. It applies to employees, contractors and anyone working on our behalf.",
        ),
        h("What we expect"),
        ul(&[
            "Treat colleagues, customers and partners with respect. Harassment, bullying and discrimination are not tolerated.",
            "Act honestly. Do not mislead customers, colleagues or anyone else.",
            "Protect confidential information about [Company], our customers and our people.",
            "Use company money and property only for business purposes.",
            "Follow the law and the policies that apply to your work.",
        ]),
        h("Conflicts of interest"),
        p(
            "Tell your manager about any situation where your personal interests could affect a work decision, for example a family member who is a supplier. Most conflicts can be managed once they are known.",
        ),
        h("Gifts and hospitality"),
        p(
            "Do not accept gifts or hospitality that could influence a decision, or that you would be uncomfortable explaining. Record anything worth more than [amount] with [contact].",
        ),
        h("Raising a concern"),
        p(
            "If you see something that breaks this code, tell your manager or [contact]. We will look into it fairly, and nobody who raises a concern in good faith will be treated badly for it.",
        ),
        h("If the code is broken"),
        p("Breaking this code can lead to disciplinary action, following our usual process."),
    ])
}

pub(super) fn acceptable_use() -> Value {
    doc(vec![
        h("Purpose"),
        p(
            "This policy explains how to use [Company]'s devices, accounts and data safely. It applies to everyone with access to our systems.",
        ),
        h("Accounts and passwords"),
        ul(&[
            "Use your own account only. Never share passwords or sign-in codes.",
            "Turn on two-step verification wherever it is offered.",
            "Use a password manager and a different password for each service.",
        ]),
        h("Devices"),
        ul(&[
            "Lock your screen when you step away.",
            "Keep devices updated and do not turn off security software.",
            "Report a lost or stolen device to [IT contact] straight away.",
        ]),
        h("Data"),
        ul(&[
            "Store work files in the approved tools: [list of tools].",
            "Do not copy customer or personal data to personal devices or accounts.",
            "Only share data with people who need it for their work.",
        ]),
        h("Acceptable personal use"),
        p(
            "Occasional personal use of company devices is fine if it does not affect your work, break the law or put our systems at risk.",
        ),
        h("Monitoring"),
        p(
            "[Company] may review the use of its systems to keep them secure, in line with applicable law. [Describe what is monitored and why.]",
        ),
        h("Reporting problems"),
        p(
            "If you think an account or device has been compromised, or you clicked a suspicious link, tell [IT contact] at once. Reporting early is always the right call.",
        ),
    ])
}

pub(super) fn leave() -> Value {
    doc(vec![
        h("Purpose"),
        p(
            "This policy explains the time off available at [Company] and how to request it. Local law may give you more; where it does, the law applies.",
        ),
        h("Annual leave"),
        ul(&[
            "Your annual leave allowance is [number] days a year, plus public holidays in [location].",
            "Request leave at least [number] days in advance where you can.",
            "Your manager approves leave, taking the team's cover into account.",
        ]),
        h("Sick leave"),
        ul(&[
            "Tell your manager as early as you can on the first day you are unwell.",
            "For absences longer than [number] days, [evidence required, if any].",
        ]),
        h("Other leave"),
        p("[Describe parental, bereavement, carer and other leave that applies.]"),
        h("How to request time off"),
        ol(&[
            "Agree the dates with your manager.",
            "Record the request in [tool or process].",
            "Arrange cover for anything urgent before you go.",
        ]),
    ])
}

pub(super) fn safety() -> Value {
    doc(vec![
        h("Purpose"),
        p(
            "Everyone has the right to go home safe. This policy sets out how we keep the workplace safe and what to do when something goes wrong.",
        ),
        h("Everyone's responsibilities"),
        ul(&[
            "Follow safety instructions and use any equipment provided.",
            "Report hazards, near misses and injuries, however small.",
            "Stop work that feels unsafe and tell your manager.",
        ]),
        h("Emergencies"),
        ul(&[
            "Know your nearest exit and assembly point: [location].",
            "First aiders: [names or how to find them].",
            "In an emergency, call [local emergency number] first.",
        ]),
        h("Working from home"),
        p("If you work from home, set up a safe workspace and tell your manager about any problems."),
        h("Reporting an incident"),
        p(
            "Report incidents to [contact or tool] within [time]. We look into incidents to prevent them happening again, not to blame people.",
        ),
    ])
}

pub(super) fn onboarding_sop() -> Value {
    doc(vec![
        h("Purpose"),
        p("Give every new starter a consistent first week, so they have what they need on day one."),
        h("Roles"),
        ul(&[
            "Hiring manager: owns the plan and the first-week check-in.",
            "[IT contact]: accounts and equipment.",
            "[People or HR contact]: paperwork and policies.",
        ]),
        h("Before the first day"),
        ol(&[
            "Confirm the start date, role and manager with the new starter.",
            "Request accounts and equipment at least [number] days before the start date.",
            "Invite the new starter to Procera so they can read and acknowledge the handbook.",
        ]),
        h("First day"),
        ol(&[
            "Welcome them and introduce the team.",
            "Check that sign-in, two-step verification and equipment work.",
            "Walk through the handbook and the policies they need to acknowledge.",
        ]),
        h("End of the first week"),
        ol(&[
            "Hold a check-in: what is going well, what is missing.",
            "Confirm the handbook acknowledgement is complete.",
        ]),
    ])
}

pub(super) fn incident_sop() -> Value {
    doc(vec![
        h("Purpose"),
        p(
            "Make sure incidents (safety, security or service problems) are reported quickly and handled the same way every time.",
        ),
        h("Steps"),
        ol(&[
            "Make the situation safe. Call emergency services if anyone is in danger.",
            "Report it to [contact or tool] as soon as you can, with what happened, when and where.",
            "The incident owner records it, decides who needs to know and starts the follow-up.",
            "Keep any evidence (photos, logs, messages). Do not delete anything.",
            "After it is resolved, the owner records the cause and what will change.",
        ]),
        h("Records"),
        p("Keep the incident record for [period], or longer when the law requires."),
    ])
}
