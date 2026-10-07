# S6A — seeded history and Rust acceptance journeys

Completed on `integration` in the main checkout. No commit was made by this agent. This implements sections 1 and 2 of `S6-scenarios-e2e.md`. `README.md` and `docs/*.md` were left to the concurrent documentation agent. During verification, a documentation-only commit (`22ab43c`) appeared; all S6A changes remain uncommitted.

`server/src/seed/scenarios.rs` now creates 26 live fictional requests and imports 12 historical records from the supplied CSV. Business changes go through the complete authenticated application router: drafts, required uploads, submission, workflow actions, payments, bookings, tasks, usage, decisions, exhibitions, complaints, imports and integration retries. There are no scenario INSERTs or UPDATEs into domain tables. Temporary seed sessions are created/deleted through the session owner's API and leave no sessions behind.

The private endpoint driver uses an ephemeral loopback receiver for actual DemoPay, mail and records HTTP calls. It runs durable jobs explicitly and advances `FixedClock` across provider delays instead of sleeping. `clock::scope` supplies a task-local override to domain APIs without an explicit time argument; outside that scope, their clock remains the ordinary system clock. Historical case creation, number allocation, events, deadlines, invoice numbering and job scheduling use that override. Time travel is not exposed by an HTTP endpoint and does not alter other tasks' clocks.

| Seeded story | Result |
| --- | --- |
| Rawson Hall | Three completed hires with inspections and completed provider refunds; one retains $50 for Extra cleaning and refunds $200. Two confirmed bookings approximately 7 and 13 days ahead; one unpaid Main Hall request approximately 19 days ahead. Dates avoid the base maintenance window. |
| Island Builders / Ben | Organisation application, replacement comment, drawing v2, separate issued development and building approvals with exact v2 evidence; completed commencement notice; linked modification under assessment. The modification has an open exhibition with an independently published, burned-redaction plan copy. |
| Planning certificates | One paid, assessed and issued; one awaiting payment. |
| Equipment | One completed Bobcat hire: estimate 4 h, elapsed 5 h 30 min, downtime 30 min, billable 5 h × $135 + $12.34 agreed expenses = $687.34. One scheduled hire. |
| Roads | Six spatially distinct reports at triage, inspection, repair, response, completed and duplicate stages, plus assisted road requests. Public map populated. |
| Confidential complaint | Alexey's complaint about Olga is completed by Ruth; independent review remains open and inherits Olga's exclusion. |
| Assisted intake | Four phone/walk-in requests with contact details and no applicant account: overdue, inspection, reopened and cancelled examples. |
| Legacy / integrations | Supplied 15-row legacy CSV applied: 12 imported, duplicates skipped and malformed row retained in the report. One delivery fails during a configured outage, is manually retried after recovery and succeeds. All 33 seeded deliveries finish accepted. |

Alexey has no open hall request. On the checked 7 October 2026 clock, 28 of the 29 whole-hall dates in weeks 2–6 are free; the existing Main Hall maintenance window occupies the remaining date, while Supper Room remains available. An acceptance test submits Alexey's own new whole-hall request after seeding.

The sample bank statement now references the unpaid seeded Main Hall request (`NSH-2026-000024`) and planning certificate (`NSH-2026-000008`). Importing it immediately after seeding produces one matched, two unmatched and one duplicate row. The smoke binds those sample references to its own newly submitted cases instead of assuming they receive the first case numbers.

`server/tests/acceptance_scenarios.rs` contains one test for each of the eight requested checks. Fixtures use real persona login, CSRF and TOTP; the new resident and the unrelated document recipient register through the API. Each journey has a separate temporary database. Journeys are serialized within the test binary because the webhook concurrency limit is process-wide; the hall race itself runs two confirmation requests concurrently.

| Acceptance check | Assertions |
| --- | --- |
| New service | Admin creates fields, required PDF, role-based workflow, publishes; a newly registered resident submits, Olga is assigned, responds and completes it. Publishing v2 preserves the earlier definition, consent, answers, reference, submission confirmation event and uploaded document. |
| Building | Text acknowledgement cannot resolve replacement; v2 resolves the action and resumes deadlines. Two separate approvals pin v2; modification links to the same project. Published PDF has empty `pdftotext` output and no fonts/text layer; the source contains the private marker and the earlier source remains unchanged. |
| Certificate | Unpaid advancement and premature issuance return 409. Payment advances to preparation; issuance still returns 409 until the specialist completes preparation, then the issued PDF downloads and the case completes. |
| Hall | Hosted checkout and signed webhook, confirmation PDFs, reschedule preview, immutable history and old PDF, $40 fee credit without a second bond, inspection, idempotent partial retention/refund. Processing refund cannot close the case; only its webhook completes it. |
| Equipment | Exact 300 billable minutes and $687.34 final invoice, estimate separation, elapsed/downtime/billable explanation and expense line, downloadable invoice, payment and completion. |
| Concurrency / money | Whole hall versus Main Hall: one 200, one 409, one confirmed and one requested booking. Same webhook and new event ID for the same provider payment leave one payment. Unclear statement stays unmatched with no case allocations; incoming money remains in suspense. |
| Intake / confidentiality | Account-free phone request reaches response with sent email and SMS. Olga gets 404 on the complaint/document; case search, finished-record search and all dashboard drill-downs exclude it, even after granting her Manager. Authenticated unrelated residents receive 404 on a copied download URL. Review inherits exclusions. |
| Recovery / handoff | Visible failed delivery, manual recovery, one remote operation/reference and rejected redundant retry. Case ZIP downloads. Actual binary `backup` and `restore-check` pass in temporary directories. Global metrics, all service breakdown counts and owner workload counts equal their filtered drill-down lengths. |

`acceptance_seeded.rs` adds comprehensive seeded catalogue assertions, ledger balance per journal entry, no negative completion durations, historical event timestamps, foreign-key integrity, scenario replay, every logical table's count after reset, no seeded sessions, visitor availability, dashboard/list agreement and immediate sample-statement usability.

Two domain defects were exposed and fixed: planning-certificate issuance previously bypassed payment and specialist preparation; the equipment invoice called billable time "actual" without distinguishing elapsed time. Issuance now checks the decision checkpoint and settlement; the invoice explicitly shows elapsed time, downtime and billable time. Existing auth/intake/integration unit fixtures now request base personas/catalogue only so their focused notification and authorization assertions are independent of the new historical catalogue.

| Final verification | Result |
| --- | --- |
| `cd server && cargo test` | Passed: 122 existing tests + 8 acceptance journeys + 1 seeded-history acceptance test = 131; zero failures. Three pre-existing documentation examples remain ignored. |
| `cd server && cargo clippy --all-targets -- -D warnings` | Passed. |
| `cd server && cargo fmt -- --check` | Passed. |
| `scripts/smoke-integration.sh` | Passed, ending `PASS integration smoke`; no flow skipped. The script also builds the frontend and runs the real seed CLI twice. |
| Separate CLI timing/count check | `seed-demo`: 1.982 s; second `seed-demo`: 1.847 s; `reset-demo`: 1.750 s. All 90 logical table counts identical; 38 cases and six bookings. |
| `git diff --check` | Passed. |

CI needs Poppler (`pdfinfo`, `pdftoppm`, `pdftotext`, `pdffonts`), as the application's exhibition publisher already does. The new redaction proof fails if these tools are missing instead of skipping. The sample CSV retains explicit 2026 references/dates and needs refreshing for another calendar year; live scenario dates are relative to the supplied clock. Providers remain the application's in-process mocks; no live vendor integration is claimed. No architecture edit or additional cross-module API request is needed.
