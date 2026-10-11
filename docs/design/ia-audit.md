# Information architecture audit (2026-10-12)

Owner feedback: "a lot of cognitive burden to learn… things are not categorised and need someone to
explain before using… confusing even for me… every persona has different flows." This audit
records what each persona sees today, as measured in the running demo (Acme, seeded data), and the
problems that feed the redesign in [ia-competitive-analysis.md](ia-competitive-analysis.md).

## What each persona sees in the sidebar today

| Persona (demo account) | Sidebar items | Notes |
|---|---|---|
| Employee (finn, Member) | Home, Notifications, Policies, Library, Announcements, Templates, *Administration:* Audit log | 7 items for someone who reads, acknowledges and gets news. "Templates" and an "Administration" section with "Audit log" (own activity) are noise. |
| Reviewer (ben, Reviewer) | Home, Notifications, Policies, Library | **No Announcements**: the Reviewer role does not include employee basics, so a reviewer is a lesser employee. Roles are not layered on a member baseline. |
| Owner/admin (olivia) | Home, Notifications, Policies, Library, Announcements, Templates, *Administration:* Overview, Members, Roles & access, People & access, Manage announcements, Campaigns, Settings, Files, Retention, Encryption, Audit log | **17 items**, one flat admin list mixing daily work (Campaigns, Manage announcements) with rare, expert settings (Encryption, Retention). |

## Problems

1. **Near-duplicates nobody can tell apart without help**
   - Members / People & access / Roles & access: three "people" pages.
   - Announcements / Manage announcements: the same objects, two places.
   - Policies / Library: policies appear in both; the difference ("applies to me" vs "all documents") isn't visible.
   - Notifications / Home "For you": two inboxes.
2. **No categories in admin.** Eleven admin items at one level, in no meaningful order; frequency
   ranges from daily (Campaigns) to yearly (Encryption key rotation).
3. **The same map for every job.** Navigation is filtered by permission, not shaped by persona. An
   author has no "my drafts / waiting for review" place; a reviewer has no review queue except Home
   tasks; HR has campaigns hidden among security settings.
4. **Jargon.** "Campaigns", "Retention", "Encryption", "People & access", "Audit log": correct,
   but the page names don't say what you do there. There is no inline explanation, and empty states
   explain only some pages.
5. **No first-run guidance.** Nothing tells a new owner what to do first (invite people, add a
   policy, ask for acknowledgement), or tells an employee what Procera is for.
6. **Role model gap.** Built-in roles other than Member are not additive on top of Member, so the
   employee basics (announcements, acknowledgements) depend on also holding Member.

## Inventory of current destinations

Home · Notifications · Notification preferences · Policies · Library (+ document, compare) ·
Announcements · Templates · Account security · Overview · Members · Person page · Roles & access ·
People & access · Manage announcements · Campaigns (+ campaign) · Settings (workspace, departments,
profile) · Files · Retention · Encryption · Audit log · System console (workspaces, quotas, health,
system audit).

## Settings (owner feedback: "too crowded and confusing")

- One **Workspace** tab lists the whole settings catalogue on a single long page: 50+ keys across
  16 internal groups (governance, notifications, campaigns, announcements, branding, locale,
  execution, security, access, directory, files, policies, reviewer, investigator, analyst,
  monitoring). Some belong to features that don't exist yet (execution, investigator, analyst).
- Settings for a feature live far from that feature: announcement settings are not reachable from
  Manage announcements; campaign defaults are not reachable from Campaigns.
- Rare, expert controls (password floors, MFA roles, retention, encryption) sit next to everyday
  ones (branding, time zone), with the same visual weight.
- Two more tabs, **Departments** (per-department overrides) and **Configuration profile**, assume the
  reader already knows the layered settings model.
- Most rows need the explainer to make sense; labels are setting names, not outcomes.
