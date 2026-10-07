# UI/UX polish 1

Branch: `integration`. Main checkout: `/Users/oleghasjanov/projects/norfolk`. No commit created.

## Before / after

| Item | Before | After |
| --- | --- | --- |
| 1. Workspace navigation | Staff had both **Overview** and **Home** pointing to `/staff`, with the same icon. Residents had both **Overview** and **My requests** pointing to `/my`. | Staff has one **Home**, opening the queues dashboard. Residents have one **My requests** navigation link; the global header omits destinations already present in resident navigation. Unused staff/resident placeholder components and route fallbacks are removed. `/admin` was checked: it has one **Overview**, which opens its useful configuration landing page, so it stays. |
| 2. Staff workflow steps | Seeded `label` values themselves contained applicant wording, including “Checking your request” and “Preparing the hall”. The API was already selecting `label` for staff. | Seeded staff labels are action wording: **Check request**, **Prepare hall**, **Confirm booking**, **Prepare certificate**, etc. Applicant labels are preserved; the existing audience-aware projection continues selecting the correct label. |
| 3. Action bar | Cancel appeared first as a solid red button. All other actions had the same secondary styling. The modal had a generic confirmation button. | Positive actions come first and use primary styling; neutral actions follow; destructive actions come last and use a shared **danger-outline** variant. Cancellation opens **Cancel request?**, explains the outcome, requires a reason, and uses a solid danger button only for the final confirmation. Opening an action clears old reasons and duplicate IDs. |
| 4. Catalogue and service page | Cards repeated the whole summary, outcome, price policy and demo caveats. The hall lead mixed a description with all hire conditions. | Each card has the name, department badge, opening sentence (at most 140 characters), a short price hint and **View service**. Card footers align within each row. Service leads use the opening sentence. Hall conditions have their own **Conditions** card. A single page footnote says “Prices are a demo copy of the FY2026-27 schedule; time targets are illustrative”. Seeded summaries, price notes and deadline labels no longer repeat the caveat. Public price rows omit repeated “demo schedule” suffixes. |
| 4a. Definition compatibility | Conditions were embedded in prose. | Rust has a defaulted `conditions: Vec<String>`; TypeScript treats it as optional. Older definitions still deserialize. The builder edits one condition per line. Existing venue definitions without a conditions list retain their remaining summary sentences in the Conditions card. Published versions and submitted snapshots are not rewritten. |
| 5. Fresh staff notifications | Jake's mobile bell showed **9+**. Tom and Helen also had accumulated historical updates. | At the end of scenario seeding, historical staff notifications are marked read, and at most three current notifications per staff persona remain unread. The fresh screenshots show Jake **3**, Helen **3**, Tom **1**, and Olga **0**. Notification history stays available. Live delivery and applicant notifications are unchanged. Seed acceptance checks cover historical reads and the three-update maximum. |
| 6. Further page audit | Submitted cases displayed **Before you submit**. Booking answers exposed `rawson-main`, object keys and UTC timestamps; select answers could show stored values. Several empty states gave no next step. The homepage example said **Booking confirmed** while its current message said confirmation was still pending. Some initial destructive controls were solid red. | Submitted case review omits pre-submission guidance; booking spaces, option labels and Norfolk dates/times read naturally. Task update types, service modules and status filters use readable labels. Resident request sections, message threads, project decisions and finance empty states explain what happens next. The homepage example now says **Confirming your booking**. Initial destructive controls share the outlined style. Task cancellation moves its reason into a confirmation dialog; booking cancellation uses a danger confirmation and requires a reason. Assisted intake appears on Home only for intake staff. |
| 7. Screenshots | Ten existing screenshots showed the issues above. | All ten are regenerated from a fresh seed. Five additional views capture the service detail, resident requests, staff Home, admin Home and cancellation dialog. Visual review and final verification are recorded below. |

## Audit coverage

Reviewed all ten original screenshots and the service, request, staff, field, finance, dashboard, public notice, project/decision, organisation and service-builder flows referenced in `docs/DEMO-GUIDE.md`. Raw workflow/task identifiers remain internal API values; user-facing status pills already humanize `waiting_on_applicant`. Staff-only configuration still exposes keys where editing them is part of the job.

The original home, calendar, booking widget, public exhibition, finance and builder layout were kept. Changes focus on copy, action hierarchy, navigation and guidance, using existing fonts, surfaces and spacing.

## Verification

All requested checks passed:

| Check | Result |
| --- | --- |
| `cd server && cargo test` | 131 tests passed: 122 unit tests and 9 acceptance tests; 3 existing doc examples ignored. |
| `cd server && cargo clippy --all-targets -- -D warnings` | Passed. |
| `cd server && cargo fmt --check` | Passed. |
| `cd web && npm run build` | Passed. Vite retains its nonblocking large-chunk advisory. |
| `cd web && npm run lint` | Passed with zero lint warnings. |
| `cd web && npm test` | 27 tests passed. |
| `cd e2e && npx playwright test` | 22 tests passed, including 15 accessibility audits, the outbox regression, 2 polish regressions and 4 demo stories. |
| `scripts/smoke-integration.sh` | `PASS integration smoke`; repeatable seed, finance, hall/equipment, approvals, field work, confidentiality, organisation access and records flows passed. |
| `cd e2e && npm run screenshots` | Passed; 15 screenshots regenerated from a fresh seed. |
| `git diff --check` | Passed. |

The new browser regressions check compact catalogue cards, the 140-character limit, conditions and the single footnote, unique workspace navigation, staff labels, readable submitted information, cancellation ordering and reason-gated confirmation. Seed tests check conditions survive loading, applicant labels remain intact, and staff unread counts stay realistic.

## Visual review

Inspected all 15 generated images, including a second inspection after the final resident-navigation and public-price-copy corrections:

- [Catalogue](../screenshots/catalogue.png): first-row summaries fit into one to three lines; actions align and are visible without scrolling through a text wall.
- [Service detail](../screenshots/service-detail.png): one-sentence lead, separate conditions, clean price rows and one footnote; applicant step wording remains intact.
- [Staff case](../screenshots/staff-case.png) and [cancellation confirmation](../screenshots/cancel-confirmation.png): one Home link, concise staff step labels, neutral actions before outlined cancellation, and a focused reason dialog.
- [Resident requests](../screenshots/resident-requests.png): one set of account destinations and a grammatically correct empty state with guidance. [Staff Home](../screenshots/staff-home.png) opens queues; [admin Home](../screenshots/admin-home.png) provides configuration links.
- [Field mobile](../screenshots/field-mobile.png), [manager dashboard](../screenshots/dashboard.png) and [finance](../screenshots/finance.png): fresh unread counts are 3, 3 and 1 respectively; layout remains readable.
- [Home](../screenshots/home.png), [hall booking](../screenshots/hall-booking.png), [calendar](../screenshots/calendar.png), [exhibition](../screenshots/exhibition.png) and [service builder](../screenshots/service-builder.png): checked typography, navigation, control consistency and visible content; no new clipping or overlap found.

Seed copy and historical-notification changes take effect on the normal fresh seed/reset. Existing published definitions and submitted snapshots remain preserved; no deployment or commit was performed.
