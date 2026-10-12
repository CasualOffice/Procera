# First-time setup

This guide takes a new Procera installation from the one-time setup token to a first published policy that
everyone has been asked to acknowledge. It assumes Procera is running and you can open it in a browser
(see [Install with Docker Compose](install-docker-compose.md)). Names in **bold** are the exact labels
you'll see on screen.

There are two kinds of administration in Procera:

- The **system console** (`/system`) runs the installation: workspaces, quotas, email and platform health.
  Its users manage workspaces, not their content.
- A **workspace** is one organisation's space: its people, documents and settings. Its owner and admins run
  it from **Admin**.

The person who sets up the installation can also own the first workspace, or invite someone else to.

## Overview

1. Create the system owner with the setup token.
2. Turn on two-step verification.
3. Set up email (or skip it for now).
4. Create the first workspace and invite its owner.
5. The owner joins and turns on two-step verification.
6. Guided setup in the workspace.
7. The first policy: write, submit, review, publish, and ask everyone to acknowledge it.

You need an authenticator app on your phone (for example 1Password, Google Authenticator or Microsoft
Authenticator).

## 1. Create the system owner

1. Get the setup token from the server log. With Docker Compose:
   `docker compose -f deploy/compose.yml logs procera | grep bootstrap_token`
2. Open `/setup` on your Procera address, for example `http://localhost:8080/setup`. The page is
   **Set up Procera**.
3. Fill in **Setup token**, **Work email**, **Your name**, **Password** (at least 12 characters; a short
   sentence works well) and **Confirm password**.
4. Click **Create system owner**.

The token works once and expires after 24 hours. If the page says **This setup token doesn't work**,
restart the server to print a new one. A rejected password or an email that already exists also uses the
token up, so check the password rules before you submit. If the page says **Procera is already set up**,
someone has finished this step: sign in instead.

## 2. Turn on two-step verification

The system console always requires two-step verification. Right after creating the account, Procera signs
you in and asks you to turn it on:

1. On **Turn on two-step verification**, click **Get started**.
2. On **Scan this code**, scan the QR code with your authenticator app (or use **Can't scan? Enter this
   key**), type the **6-digit code** and click **Verify and turn on**.
3. On **Save your recovery codes**, **Download** or **Copy** the codes and keep them somewhere safe. Each
   one lets you sign in once if you lose your phone.
4. Click **I've saved them, continue**. You go straight on to the console, without signing in again.

## 3. Set up email

You land on **Finish setting up** (`/system/setup`), which has two short steps: **1. Email (optional)**
and **2. First workspace**.

Procera emails invitations, reminders and password resets. Without email they wait in a queue, and
nothing is lost: they go out once email works. But the workspace owner joins through an emailed
invitation, so set up email now if you can.

On **Set up email**, enter your mail provider's details:

| Field | What to enter |
|---|---|
| **SMTP host** | The server name only, such as `smtp.example.com`. |
| **Port** | Filled in from the encryption choice: 587 for STARTTLS, 465 for implicit TLS. |
| **Encryption** | **STARTTLS (required)**: delivery stops if the server doesn't offer encryption. **Implicit TLS**: encrypted from the first byte. **None (only for a relay on this server)**: allowed only when the host is `localhost`. |
| **Username** / **Password** | Leave empty if the server doesn't ask for a sign-in. The password is stored encrypted and never shown again. |
| **From address** | An address your mail server is allowed to send from. |
| **Sender name** | Shown as the sender. Workspaces can set their own. |
| **Reply-to address** | Optional. |

Click **Save and continue**, then **Send test email** to check delivery end to end, and **Continue**.
Saving asks you to confirm it's you (your password or a code) if you haven't verified recently.

- If the server sets `PROCERA_SMTP_URL`, the step says **Email is already set by the environment**.
  There's nothing to enter: send a test email and continue.
- To do it later, click **Skip for now**. You'll find the same form in the system console under
  **Email & platform**, which also shows the public address, file storage and malware scanning in use.
- The page has DNS advice (SPF, DKIM, DMARC) to help email reach inboxes. Procera doesn't sign messages
  itself.

## 4. Create the first workspace

On **Create your first workspace**, click **Create workspace**. The **New workspace** dialog asks for:

- **Workspace name**, for example your company's name.
- **Slug**: identifies the workspace in addresses. It is suggested from the name; lowercase letters,
  numbers and hyphens, 3 to 63 characters.
- **Configuration profile**: pre-sets security, review and retention defaults. Every setting can be
  changed later.
  - **Starter**: under 50 people. Two-step verification for owners and admins; the owner publishes
    without review.
  - **Standard**: most organisations. Invite-only, independent review for controlled processes.
  - **Enterprise**: many departments. Two-step verification for everyone, two-person rule, access reviews.
  - **Regulated**: pharma, finance, public sector. The dialog's hint mentions Part 11 signatures and
    Separation mode; what the profile sets today is two-step verification for everyone, a review with two
    reviewers for every document, and re-acknowledgement on every publish.
- **Owner email**: the person who will own and set up the workspace. It can be your own address.

Click **Create workspace**. The confirmation says **We invited … to own …**, and the owner gets an
email invitation. If email isn't set up, the invitation waits in the queue and goes out once you set up
email in **Email & platform**.

Click **Go to the system console** to see the workspace under **Workspaces**. **Quotas & usage** shows
seats and storage per workspace, and the default limits for each profile.

## 5. The owner joins

The owner opens the link in the invitation email. The page is **Join** followed by the workspace name.

- **New to Procera:** fill in **Your name**, **Password** and **Confirm password** and click
  **Create account and join**.
- **Already has an account** (for example the system owner inviting themselves): click
  **Sign in to accept**, or **Join** followed by the workspace name if already signed in with that email.

Every profile requires two-step verification for workspace owners, so a new owner sees
**Turn on two-step verification** next. It's the same three steps as in step 2. After
**I've saved them, continue**, the owner is in the workspace, on **Home**.

Invitation links expire. If one doesn't work, a workspace admin can **Resend** it from **Admin ›
Members**; the earlier link then stops working.

## 6. Guided setup in the workspace

Home shows a **Set up** (workspace name) checklist for owners and admins (it disappears when every step is done, or you can
hide it). Click **Start setup** to open four short steps. Each one saves as it goes, and you can
**Skip for now** and come back from Home.

1. **Your workspace**
   - **Workspace name** and **Accent colour** (used for links and highlights; Procera adjusts it so text
     stays readable). Add a logo later in Settings.
   - **How documents get approved**:
     - **Someone else reviews every document** (recommended): the author can't approve their own work.
     - **Only controlled documents**: everyday documents publish straight away.
     - **No review**: authors publish directly.
   - **Time zone**: due dates, reminders and digests follow it.
   - Click **Save and continue**.
2. **Starting point**: starter packs of reviewed documents. They install as drafts for you to adapt,
   review and publish. Click **Install and continue**. More packs are under **Admin › Templates**.
3. **Departments**: type one per line (for example People, Finance, Engineering, Sales). They're used to
   target documents, campaigns and announcements, and to scope roles. Click **Add … and continue**.
4. **Invite people**: an email and a role per person: **Member**, **Reviewer**, **Compliance** or
   **Admin**. Everyone can read and acknowledge; pick a role for people who review or manage. Invite at
   least one **Reviewer** if documents need review. Click **Invite … and finish**.

The Home checklist also links to the remaining steps: **Choose who reviews and who manages**
(**Admin › Roles & access**), **Publish your first policy**, **Ask everyone to acknowledge it** and
**Require two-step verification for admins** (**Admin › Settings**).

## 7. Your first policy

### Write it

1. Open **Library** and click **New document** (or use a draft from a starter pack).
2. In **New document**, choose **Policy**, enter a **Title** (for example "Acceptable Use of IT"), and
   tick **Controlled** if it must always have an independent review. Click **Create draft**.
3. Click **Edit**, write the content (type `#` for a heading, `-` for a list), and click **Save draft**.
   You can add files under **Attachments** with **Attach a file**.
4. Optional: under **Applies to**, click **Change** to choose who the policy applies to (**Everyone** or
   **Specific people** by department, group, role or location). It then shows in their **Applies to me**
   list.

Nobody else sees a draft until it's published.

### Submit it for review

Click **Submit for review**. The status changes from **Draft** to **In review**. There's no reviewer to
pick: everyone who is allowed to review sees the task on Home under **Needs you** and gets a notification.
While it's in review, you can **Withdraw** it back to a draft.

On the Starter profile (or with **No review**), submitting approves the draft straight away, unless it's
marked **Controlled**.

### Review it

The reviewer opens the task (**Review**) from Home or from the notification and reads the document. Then:

- **Approve**: the status becomes **Approved** once enough reviewers have approved (the **Approvals**
  property shows how many so far).
- **Request changes**: the dialog asks **What should change?** and sends the draft back to the author.

Authors can't review their own version. If the reviewer hasn't verified recently, Procera asks them to
**Confirm it's you** first.

### Publish it

Someone allowed to publish clicks **Publish** on the approved version. In the **Publish v1** dialog:

- **Goes live (optional)**: leave it empty to publish now, or pick a date and time to schedule it.
- **Note (optional)**: for later versions this becomes **What changed?** (required), with a
  **Type of change** of **Major** or **Minor**.

Click **Publish now** (or **Schedule**). The status becomes **Published** (or **Scheduled**) and it's
the version everyone reads. Publishing doesn't ask anyone to acknowledge it by itself; that's the next
step. To change it later, click **New version**.

### Ask everyone to acknowledge it

1. Open **Campaigns** and click **New campaign**.
2. Pick the **Document** (only published documents are listed). The **Name** defaults to
   "Acknowledge" followed by the title.
3. Choose the **Audience**, tick **Also ask people who join later** if new joiners should be included,
   and optionally set **Due on** (empty uses the workspace default, counted from launch).
4. Click **Create draft**. The campaign page shows how many people will be asked and the statement they
   confirm (by default "I have read and understood this document and will follow it.").
5. Click **Launch**. People are notified.

Each person sees **Acknowledge "…"** on Home, clicks **Read and acknowledge**, reads the document and
clicks **I acknowledge**. On the campaign page you can follow **Acknowledged**, **Pending** and
**Overdue**, **Remind pending**, download the **Evidence CSV**, and **Close** the campaign when you're
done. Reminders and manager escalation also run automatically according to the workspace settings.

Every step above is recorded in the audit log (**Admin › Audit log**).

## Where to go next

- [Configuration reference](configuration.md): server settings, settings in the system console and in
  workspace Settings, and security notes.
- **Admin › Settings** in the workspace: sign-in and security, documents and reviews, acknowledgements,
  notifications, retention and more.
