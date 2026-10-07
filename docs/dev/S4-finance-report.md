# S4 finance handoff

All work is uncommitted and confined to the finance feature, mock payment provider, migration 0401, and the explicitly requested statement fixture.

## Implemented API

| Endpoint | Behaviour |
|---|---|
| `GET /api/admin/prices` | Catalogue and complete version history; finance or sysadmin |
| `POST /api/admin/prices/{code}/versions` | Schedule integer-cent rates today or later; reject changes affecting issued invoices; preserve later scheduled versions |
| `GET /api/cases/{id}/money` | Authz projection of invoice lines, credits, confirmed receipts, bonds, decisions, itemised retention, refunds, sessions and available credit; staff additionally receive evidence, allocation history and case journal |
| `POST /api/cases/{id}/checkout` | Applicant/staff hosted DemoPay session; redirect never confirms payment |
| `POST /api/webhooks/demopay` | HMAC-SHA256, five-minute tolerance, event/payment deduplication; invalid signatures stored separately and answered with 400 |
| `POST /api/finance/statements` | `{filename,csv}` UTF-8 incoming-transfer CSV; file SHA dedupe, transaction dedupe, exact unambiguous reference/balance auto-match, partial suggestions and suspense accounting |
| `GET /api/finance/unmatched` | Unmatched rows and receipt-document evidence, projected by case scope |
| `GET /api/finance/cases?q=` | Scoped number/name/title lookup, revisions and outstanding invoices for matching/reallocation |
| `POST /api/finance/statement-rows/{id}/match` | `{case_id,invoice_id?,expected_revision}`; fees first, partial balances and excess credit |
| `POST /api/finance/statement-rows/{id}/ignore` | `{note}`; preserves actual received money in suspense; staff-only event for a suggested case |
| `POST /api/finance/allocations/{id}/reverse` | `{reason,expected_revision}`; preserves reversed allocation history and reverses its receivables posting into customer credit |
| `POST /api/finance/payments/{id}/allocate` | `{case_id,invoice_id?,expected_revision}`; apply available credit to the same applicant's request; validates access to both source and destination |
| `POST /api/finance/payments/counter` | `{case_id,invoice_id,amount_cents,receipt_no,expected_revision}`; confirmed cash/EFTPOS receipt with unique receipt number |
| `GET /api/finance/deposits?status=awaiting_decision` | Fully paid bonds after event end and completed inspection |
| `POST /api/cases/{id}/deposit-decision` | Atomic refund reservation, itemised retention and reason; accepts Idempotency-Key and expected_revision |
| `GET /api/finance/refunds` | Processing, failed and completed refunds with reasons and bank references |
| `POST /api/finance/refunds/{id}/confirm-bank` | `{bank_reference,expected_revision}`; completion only after finance confirms transfer |
| `POST /api/finance/refunds/{id}/retry-bank` | `{reason,expected_revision}`; recover a failed provider refund through a bank transfer, retaining its reservation |
| `GET /api/finance/overview` | Unmatched transfers, pending/failed refunds, bonds awaiting decision, outstanding invoices and receipts by Norfolk local date |
| `GET /api/finance/ledger?case_id=` | Scoped journal and trial balance |

The existing `finance::api` signatures are implemented. Generic pricing reads the frozen definition, parses decimal quantities without floats, and uses the submission's Norfolk local date. Hourly quotes use exact minutes. Invoice PDFs call `pdf::simple_document` and `documents::api::attach_generated` in the caller's transaction, storing the immutable download version.

Every money event has one balanced, idempotent journal entry. Estimates have none. Credit notes identify their original invoice and line (`calc.original_line_id`). Repricing credits unused charges, preserves unchanged bond allocations, applies available receipts to revised charges, and leaves surplus as customer credit. Consumed hires and decided bonds cannot be repriced. Deposit decisions reserve their full refundable amount under BEGIN IMMEDIATE and a unique deposit-line constraint. Refunds post completion only on a signed provider event or bank confirmation.

Receipt uploads are evidence documents and never payments. Bank imports accept positive incoming receipts, not withdrawals. Statement dates determine the receipt's Norfolk local date; journal timestamps record when it was posted.

## DemoPay

Separate provider tables store sessions, payments implicit in paid sessions, refund reservations and webhook delivery attempts. The hosted HTML checkout has success, decline, duplicate-webhook and 20-second delayed-webhook buttons. API calls require X-Mock-Key. Refunds return pending and complete through a durable job approximately three seconds later. Amounts ending in 13 cents fail. Delivery runs over real internal HTTP with signed bodies; the platform durable queue supplies retries/backoff and the provider tracks every attempt. The browser redirect never touches council payments.

## Pages

The `finance.money` case panel serves residents and staff: charges, refundable bonds, payment processing with polling/backoff, confirmed receipts, itemised bond decisions and true refund statuses. Finance actions cover counter receipts, bond decisions, reversal and reallocation to another request of the same applicant. Staff see evidence, allocation history and case ledger.

Registered routes: `/staff/finance`, `/staff/finance/statements`, `/staff/finance/unmatched`, `/staff/finance/deposits`, `/staff/finance/refunds`, `/admin/prices`. `/staff/finance/prices` additionally makes prices reachable for finance-only users under the current platform admin guard. Components use the shared UI, labelled fields, field errors, modal focus handling, live payment feedback, scrollable tables and Norfolk dates. All price displays carry the required demo schedule note.

## Decisions and merge requests

- Additional owned tables: finance payment credit/suspense balances and invoice-document version links. Provider tables remain independent. No shared schema or platform files changed.
- CSV file upload uses JSON `{filename,csv}` after reading the selected file in the UI; no new multipart dependency or parallel upload platform.
- Future scheduling can split a finite interval while keeping an already-scheduled later rate. Rates used by issued invoices in the affected period cannot be changed.
- Cross-module calls use the existing documents, records and workflow APIs; tests intentionally avoid requiring their current stubs to be implemented. Full merged case workflows/PDF attachment need verification after those branches land.
- Platform request: permit finance users to reach `/admin/prices` without granting other admin capabilities. The current `/admin` guard admits only sysadmin/manager; finance users have the working `/staff/finance/prices` alternative.
- S6 scenario request: align the statement fixture with the seeded awaiting-payment case. `server/seed-data/statements/demo-statement.csv` expects NSH-2026-000001 to owe $365 (main hall + bond), and NSH-2026-000002 to have more than $50 outstanding. The final row duplicates DEMO-BANK-001. If S6 assigns different case numbers, update this fixture during orchestration.
- No new cross-module functions requested.

## Validation

Rust tests cover rounding, minute pricing, estimates, immutable rates, scheduled intervals, frozen definition pricing, duplicate events and provider payment IDs, bad signatures and tolerance, duplicate statement files and rows, ambiguous references, partial payments/remainders, excess credit, reversal/reallocation, unchanged bond preservation, refund concurrency, settlement states, failed refund recovery and scoped finance queues. Every database money scenario asserts the global debit/credit sum and each entry's balance. A real HTTP provider test verifies redirect isolation, failed-delivery retry, HMAC transport, pending asynchronous refund and webhook-only completion.

Final checks passed: `cargo test` (54 tests, including 22 finance tests; 3 existing documentation examples ignored), `cargo clippy -- -D warnings`, `npm run build`, `npm run lint`, `cargo fmt -- --check` and `git diff --check`. Rust checks use CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0 and CARGO_INCREMENTAL=0 to avoid the shared disk-space limit encountered during the first debug-symbol build.
