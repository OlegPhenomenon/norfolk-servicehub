# Presenting Norfolk ServiceHub

A script for walking someone through the demo — at https://norfolk.shelfcompass.com or a local `seed-demo` install. Everything is fictional; every result is produced by the actions you take, not drawn in advance.

## Before you present

- **Sign in on `/demo`.** One click per persona. Staff personas complete a TOTP step — copy the current code from the **Demo authenticator** box on the same page. Codes rotate every 30 s.
- **Two windows help.** A second browser or an incognito window lets you keep a resident and a staff member signed in at once and pass the case between them.
- **DemoMail** (`/mock/mail`, linked in the demo banner) shows every email/SMS the system "sent". Good for "the resident would have received this".
- **The banner shows the next data reset.** Everything you create is wiped when it fires — finish paid/refund stories before then.
- **Nothing is real.** Payments are DemoPay test-mode, people are fictional, prices are a demo copy of the FY2026-27 schedule.

| Persona | Role in stories |
|---|---|
| **Alexey Turner** | Resident — books the hall, reports a pothole, orders a certificate, complains |
| **Ben Carter** | Island Builders Pty Ltd — building application, equipment hire, organisation account |
| **Olga Novak** | Customer Care intake — checks new requests, records phone/walk-in requests, confirms bookings |
| **Priya Nair** | Planning & Building specialist — assesses, comments, **issues decisions** (holds authority) |
| **Tom Becker** | Finance — invoices, bank statements, bond decisions, refunds |
| **Jake Rowe** | Works Depot field worker — tasks on his phone, records actual equipment hours |
| **Helen Ford** | Manager — dashboard, reassignments, grants decision authority, can publish exhibitions |
| **Ruth Adams** | Complaints officer — the only staff role that sees confidential complaints by default |
| **Mark Ellis** | Systems administrator — services, prices, users, integrations, backups; **sees no case content** |

---

## Story 1 — Alexey hires Rawson Hall (~2 min)

*Personas: Alexey → Olga → Alexey → Olga → (Jake) → (Tom)*

1. **`/demo` → "Sign in as Alexey".** Open **Services → "Application for Hire of a Council Premises — Rawson Hall"**. Point out the service page: what you get, documents to prepare, the price card (Main Hall / Supper Room / whole-hall fees and the $250 bond), and the steps the request will go through.
2. **Start request.** Pick a date, time and space — the booking widget shows live availability (whole-hall bookings block both rooms). Fill the required fields, submit. The green banner says the request is **received, not confirmed** — and gives the `NSH-…` reference.
3. **Switch to Olga** (`/demo`, TOTP from the authenticator). **Staff workspace → New** queue → open the case. Her view shows the frozen answers, documents, deadline targets, a Messages thread, and staff-only Internal notes. **"Complete this step"** advances to payment — an invoice with separate hire-fee and bond lines is issued automatically.
4. **Back as Alexey**: My requests → open the case → **Money** tab → **Pay**. The browser lands on the DemoPay checkout (branded *TEST MODE*). Choose **"Pay with test card"** → redirect back → the case now reads *payment received — confirming your booking*. The redirect itself never confirms money; the signed webhook does.
5. **Olga** opens the case → **Booking** tab → **Confirm booking** (enabled only because it's paid and conflict-free) → advance. A booking confirmation PDF is generated; download it from the Booking tab.
6. If you have time: **Jake** (`/staff/field`) completes the hall-prep and post-event inspection tasks; **Tom** records the bond decision on the case's Money tab — e.g. retain $50 *Extra cleaning*, refund $200. The refund reads **processing** until the provider's refund webhook lands, then *completed* — only then does the case close.

**Point out:** the same timeline is visible to both sides; a reschedule (Booking → Reschedule) previews the fee difference, keeps the old revision and its PDF, credits the old fee and leaves any surplus as customer credit — never silently absorbed.

## Story 2 — Building approval with a revised drawing (~2 min)

*Personas: Ben (Island Builders) → Olga → Priya → Ben → Priya → Helen*

1. **Ben** → Services → *"Application for Development and/or Building Approval"* → submit with plans attached. It's an **organisation case** — any Island Builders staff member could follow it.
2. **Olga** checks intake and advances. **Priya** opens the case → **Documents** → comments on the site plan asking for a replacement. The applicant sees a required action; the response deadline pauses.
3. **Ben** uploads **version 2** on the same document. The comment resolves, the clock resumes — and version 1 stays in history.
4. **Priya** → **Decisions** tab: prepares the *development approval* and the *building approval* as **two separate decisions**, pinning evidence to version 2, then issues both. The applicant downloads signed PDFs that name the exact evidence versions.
5. Show the **building project** page (`/staff/projects/…`, linked from the case): the application, both approvals, a later **commencement notice** and an in-flight **modification** are linked — nothing merges into one checkbox.
6. **Exhibition** (if seeded, or create it): Priya → `/staff/exhibitions` → draft a notice, pick the site-plan version, draw redaction rectangles over the private marker — then **Helen** (a second staff member — the preparer can't publish their own) approves and publishes. The public copy at `/notices` is a new PDF rendered from pixels: `pdftotext` finds nothing, there is no black-box-over-text trick.

**Point out:** issuing needs explicit *decision authority* (Helen granted Priya's; Mark the sysadmin can't issue anything). A refusal is issued the same way and recorded with its reasons.

## Story 3 — A planning certificate (~1 min)

*Personas: Alexey → Olga → Tom/DemoPay → Priya*

1. **Alexey** → Services → *"Planning Certificate — Section 98 Planning Act 2002"* → enter the Portion reference → submit.
2. **Olga** advances intake. The payment step auto-issues the **$181.13** invoice (demo-copy price). Try pressing *Complete this step* before paying — the guard blocks it and says why.
3. **Alexey** pays on the Money tab → the case auto-advances to specialist preparation.
4. **Priya** prepares the certificate and issues it: Alexey downloads the issued PDF; the case closes with the exact issued version on file.

**Point out:** the fee was invoiced on submission but *acceptance* waited for confirmed money; the certificate is a decision document with recorded evidence, not just a status flag.

## Story 4 — Equipment hire billed on actual hours (~2 min)

*Personas: Ben → Olga → Jake → Tom → Ben*

1. **Ben** → *"Application for Hire of Council Plant / Equipment"* → the equipment widget: what's needed, requested hours (4), preferred date, site. Submit.
2. **Olga** advances intake → on the **Equipment** tab schedules a real plant item (e.g. the Bobcat at $135/h), operator Jake and the time window.
3. **Jake** at `/staff/field` sees only the job — not Ben's documents or notes. He records the job card: actual 07:30–13:30 with 30 min downtime and $12.34 agreed expenses, completes the task.
4. **Tom** (or the intake officer) approves usage → the final invoice is created immediately: **330 minutes × hourly rate + expenses**, with the basis written on the invoice. Compare with the earlier 4-hour *estimate* — estimates never become debt.
5. **Ben** pays the invoice → done.

**Point out:** the billed amount comes from approved actual minutes (integer arithmetic, `round_half_up`), not from what was requested — the invoice explains the difference itself.

## Story 5 — A road issue, reported and answered (~2 min)

*Personas: Alexey → Olga → Jake → Olga*

1. **Alexey** → *"Report a Road Issue"* → drops a point on the map (or types coordinates — the widget is keyboard-accessible), describes the pothole, adds a photo. Submit.
2. Open `/map` in an incognito window: the pin appears on the **public map** — id, category, location, status, reported date. No name, no photo, no free text leaves the case.
3. **Olga** triages → an inspection task is created. **Jake** completes *road inspection*, then *repair* (record what was actually done, add a photo).
4. **Olga** → **Road response** tab → edits and issues the written response → the case completes and Alexey gets a real answer letter.
5. Bonus: staff can **Close as duplicate** and link it to the original report — duplicates stay recorded, they aren't deleted for prettier statistics.

**Point out:** a completed road case quietly produces an outbound records delivery (`/admin/integrations` shows it accepted by the mock system).

## Story 6 — A confidential complaint (~2 min)

*Personas: Alexey → Ruth → (Olga for contrast) → Alexey*

1. **Alexey** → *"Make a Complaint"* → describe it, name the staff member concerned → submit.
2. **Ruth** sees it in her queue. On the **Confidential feedback** panel she adds the subject (e.g. Olga) — the subject is now excluded by an explicit access denial, over and above the case already being confidential by default for complaints.
3. **Contrast:** sign in as **Olga** — the case URL 404s, search finds nothing, and even Mark the sysadmin sees diagnostics only. Outbound mail about it says just *"There is an update on your feedback NSH-…"*.
4. **Ruth** investigates and issues the complaint response letter → completed.
5. **Alexey** disagrees → **request review** on the same case → a new linked complaint is created (different officer), keeping the history connected — no fresh email thread.

**Point out:** complaints never appear on the public road-issue map, and a complaint about a staff member structurally cannot land on that person's desk.

## Story 7 — A new service, built by an administrator (~2 min)

*Personas: Mark → new resident → Mark*

1. **Mark** → `/admin/services` → **Create service** — name, slug, category, department, module *generic*. In the editor: add fields (text/select/checkbox, conditional *show only when* rules), required documents, workflow steps (review → optional payment → assessment → done), deadline targets, price lines. Or shortcut: **"Suggest from a council form (AI, mock)"** on a real council PDF produces a draft staff must check — AI never invents rules, pricing or deadlines.
2. **Preview** tab → type sample answers → *Check sample answers* validates the definition against the same server rules residents hit. Then **publish**.
3. Register a **new resident** (`/register` — name, email, password) — the new service is already in the catalogue. Submit a request end to end: intake → review → result.
4. Back as **Mark**: open the service → **New draft from published** → change a field → publish v2. Open the v1 request: it still shows the original fields, answers and confirmation — the frozen snapshot never changes retroactively.

**Point out:** no developer, no code edit, no deploy. And "dog registration" sits in the list as a ready-made draft if you want a safer publish target.

---

## Things to try that should fail safely

Each of these is a good "sceptical audience member" moment. Expected result in brackets.

- **Double booking.** Alexey requests the *whole hall* and Ben requests *Main Hall* for an overlapping time; both pay. As Olga, confirm both — **exactly one confirms**; the other gets a conflict error and keeps their paid, unconfirmed booking (staff can reschedule or refund; money is never lost silently). *[409 conflict]*
- **Duplicate payment webhook.** On the DemoPay checkout, choose **"Pay — and deliver the webhook twice"**. One payment is recorded; the second delivery is stored as a deduplicated attempt, not a second receipt. *[one payment, one provider payment id]*
- **Olga opens the complaint about her.** While a complaint naming Olga is active, sign in as Olga and visit its case URL, or search its number. Also try downloading one of its documents via a copied link. *[404 everywhere — case, documents, search, dashboard drill-down]*
- **A stranger with a copied document link.** As Alexey, copy a document download URL from his case; sign in as Ben and open it. *[404 — downloads recheck case access, never trust the URL]*
- **Unclear bank transfer.** As Tom, `/staff/finance/statements` → import `server/seed-data/statements/demo-statement.csv`: exact matches reconcile, the unclear "Thank you" transfer stays in **Unmatched transfers** (suspense) with no money posted to a case, and re-importing the same file is rejected as a duplicate. *[unmatched stays unmatched; duplicate file → 409]*
- **Provider outage mid-delivery.** As Mark, `/admin/integrations` → enable **Simulate outage** on a mock system → close a case or issue a decision → the delivery row shows **failed**; disable the outage → **Retry** → exactly one record exists on the mock remote. *[failure visible, retry idempotent]*
- **The sysadmin who "configures" a decision.** Mark can manage users, services and prices — but opening any case's content or issuing a decision is denied. *[403/404 — authority comes only from a manager's explicit grant]*
- **Backdating a price.** As Tom or Mark at `/admin/prices`, try to schedule a price change that would alter an already-issued invoice. *[rejected — issued invoices keep their rates]*

## If something looks stuck

- **Staff asked for a code** — the Demo authenticator on `/demo` shows live staff codes; pick the row for your persona.
- **"Complete this step" greyed out or blocked** — read the amber guard message on the case; it names exactly what's missing (unpaid invoice, open required action, unconfirmed booking).
- **Payment "processing" forever** — the webhook worker may still be delivering; DemoPay also has a *delayed webhook* button for showing this state on purpose. The Money tab polls until confirmation lands.
- **The site suddenly looks empty** — the scheduled demo reset fired; sign back in via `/demo`.
