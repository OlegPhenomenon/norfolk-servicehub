# Integration pass 2 — finance and complete money stories

Completed on `integration`, 7 October 2026. No commit was made; `deploy/` was untouched.

## Changes and defects fixed

| Area | Finding and result |
| --- | --- |
| Equipment price mapping | The merged S4 seed already has the requested names/rates: Bobcat `EQUIP_EXCAVATOR_HOUR` $135/h; Volvo Loader `EQUIP_BACKHOE_HOUR` $230/h; Hino Truck `EQUIP_TIPPER_HOUR` $110/h; Cat Steel Drum Roller 8T `EQUIP_ROLLER_HOUR` $211/h. Verified over HTTP and by calculating a final invoice for each. No redundant seed change was needed. |
| Equipment invoice reuse | `ensure_invoice_for_step` already leaves an issued final invoice intact. Each HTTP equipment journey asserts that approval replay and payment-step entry leave exactly one final invoice, with the usage approval's invoice ID. Estimates remain separate and do not create debt. |
| Equipment → Money / checkout return | The panel key and links already used `finance.money`, but `CasePage` ignored the URL's `tab` parameter. Connected the case tabs to search parameters. Equipment links, checkout return URLs and direct Money links now select Money. Browser checks verify both the direct URL and Equipment link. |
| Price management | Chose one canonical page: `/admin/prices`. The admin layout admits finance specifically on that path, including its trailing-slash form; other admin pages retain their existing role gate. Finance navigation links there. Removed the duplicate `/staff/finance/prices` route. Tom's API access and rendered price page both pass. |
| Map | Leaflet uses `https://tile.openstreetmap.org`, while the CSP allowed only subdomains of that host. Corrected the CSP at the platform boundary. Map tiles now load without CSP errors. |
| Direct page loading | Lazy routes emitted React Router's missing `HydrateFallback` warning. Added the shared loading component as the root route's hydration fallback. The final browser run has no warnings. |
| Workspace regression test | Its panel assertion matched tab buttons' `aria-controls`, so it passed without mounting the selected panel. Assert the actual rendered tabpanel ID instead. Added the application's real `ToastProvider` to the test wrapper, which became necessary once URL-selected panels actually mounted. |

The merged finance, documents, operations and workflow implementations successfully handle payment-triggered advancement, task completion, decision issuance/refusal and refund-triggered closure. No additional backend domain fix was needed for those flows.

## HTTP smoke coverage

`scripts/smoke-integration.sh` now has no finance skips or expected invoice-stub errors. It builds the web app, runs the real `seed-demo` CLI twice, compares logical table counts (excluding SQLite FTS shadow storage), and exercises business commands using curl with resident/staff sessions, CSRF and staff TOTP.

The new `server/examples/smoke-server.rs` uses the complete application router, worker and scheduler, with an injected clock reading a local offset file. It requires demo mode and binds only to loopback. This lets the test book approximately three weeks ahead and later cross the event date without weakening the production checks against early bond decisions or future actual usage. Time continues ticking for asynchronous provider jobs. Time travel is not exposed in the production binary or through an HTTP endpoint. After advancing 29 days, the smoke signs in again because sessions expired.

Every booking, invoice, payment, task, decision, usage and refund action goes through HTTP. SQLite access in the smoke is read-only: seed counts, duplicate webhook delivery attempts and the single provider-payment assertion. DemoPay forms are parsed from its actual HTML; curl posts the selected form and follows its redirect. Signed payment/refund webhooks arrive over the configured internal HTTP URL through the durable worker.

Verified flows:

- Alexey requests the whole hall 21 days ahead. The submission notification and history explicitly say it is unconfirmed. Olga advances intake; the invoice contains a $155 fee and separate $250 bond. Duplicate DemoPay delivery creates one payment and auto-advances to confirmation. Olga confirms; the confirmation PDF downloads.
- Olga moves the booking to Main Hall on the following day. Preview shows the fee difference. Old booking revision and PDF remain accessible; a new confirmation downloads. Repricing preserves the original bond, credits the old fee, applies money to the new $115 fee and leaves $40 customer credit. Jake's preparation task advances to inspection.
- After the event, Jake completes inspection. Tom records itemised $50 retention for **Extra cleaning** and a $200 provider refund. Decision replay is idempotent. The refund initially says processing, the case stays open, and manual advancement returns 409. Only the refund webhook completes the refund and case. The exact `record.case_closed` delivery is accepted; the case ledger balances.
- Two different residents submit overlapping whole-hall and Main Hall requests and pay both. A thread barrier starts confirmation requests concurrently. Exactly one booking confirms and the other returns 409, remaining requested.
- The unchanged statement fixture imports as **1 matched / 2 unmatched / 1 duplicate**. The partial payment has the expected case suggestion, is manually matched and leaves $315 owed. The unclear $90 transfer stays in suspense. Importing identical bytes under another filename returns 409.
- Planning certificate: unpaid advancement returns 409; DemoPay payment advances to specialist preparation; Priya issues the certificate; its PDF downloads and the case completes. Receipt, decision and closure deliveries are all accepted. A second paid certificate with an issued refusal closes as refused.
- Each fleet item: estimate 4 h; schedule plant and Jake; record actual 07:30–13:30 with 30 min downtime and $12.34 agreed expenses; complete the job; Tom approves; one final invoice enters payment; invoice PDF and explanatory basis are present; payment closes the case and its closure delivery is accepted.

| Plant | Final amount: `round_half_up(330 × hourly_rate_cents / 60) + 1234` |
| --- | ---: |
| Bobcat | $754.84 |
| Volvo Loader | $1,277.34 |
| Hino Truck | $617.34 |
| Cat Steel Drum Roller 8T | $1,172.84 |

The smoke checks each journal entry and the global trial balance are zero-sum. It retains the pass-1 building replacement/two-approval, project-link, road-task/response, confidential complaint/review, organisation revocation, historical import/disposal and session-revocation checks.

Final fresh temporary-database output:

```text
ok   seed-demo twice: base table counts identical
ok   Tom can manage prices; all four fleet codes/names/rates agree
ok   seeded CSV: 1 matched / 2 unmatched / 1 duplicate; partial matched manually; unclear transfer stays in suspense; same file → 409
ok   planning certificate → unpaid advance blocked → DemoPay → specialist → issued certificate PDF → completed → records delivered
ok   paid certificate with specialist refusal closes as refused, with issued decision retained
ok   Alexey: unconfirmed submission → fee + separate bond → duplicate DemoPay delivery produces one payment → Olga confirmation PDF → reschedule/history/fee credit → Jake prep
ok   two residents, whole hall/Main Hall overlap, both paid: parallel confirmation → exactly one confirmed, one 409
ok   Alexey: Jake inspection → Tom itemises $50 Extra cleaning / $200 refund → processing cannot close → DemoPay refund webhook → completed → records delivered; case ledger zero-sum
ok   all four plant rates: estimate 4 h → actual 07:30–13:30 / 30 min downtime → one final invoice 330 min × rate / 60 + $12.34 expenses → PDF/basis → paid → completed; global ledger zero-sum
ok   building intake → replacement request → text reply keeps action → v2 clears action/resumes clock → two Priya approvals → completed
ok   canonical original_approval/project_reference answers link modification and both notices to the building project
ok   road report → Jake inspection done → Jake repair done → response letter → closed → integration delivered
ok   confidential complaint submitted/assigned → Olga 404 → generic outbound → completed → independent review copies exclusions
ok   revoked organisation member immediately receives 404 on the real document download; owner retains access
ok   historical legacy import preserves Norfolk dates; replay is idempotent; repeat upload detects exact duplicate; S5 disposal succeeds
ok   S5 user deactivation immediately invalidates the resident session
PASS integration smoke
```

## Acceptance checks

| Command | Result |
| --- | --- |
| `cd server && cargo test` | 100 passed, zero failures; 3 existing documentation examples ignored |
| `cd server && cargo clippy --all-targets -- -D warnings` | Passed, including the new smoke server example |
| `cd web && npm run build && npm run lint && npm test` | Passed; zero lint warnings; 27 tests across 3 files |
| `scripts/smoke-integration.sh` | Passed on a fresh disposable database, no skip lines |
| `git diff --check` | Passed |

## Headless browser verification

Also ran the ordinary production `servicehub seed-demo` twice and `servicehub serve` with a separate temporary database, no injected clock. Created seven module fixtures through HTTP. Headless Chromium (Playwright 1.63.0) visited 27 pages, with listeners for page exceptions, console errors/warnings, failed requests and unexpected same-origin HTTP 4xx/5xx responses.

Covered `/`, `/services`, `/services/rawson-hall-hire`, `/demo`, `/my`, `/staff`, `/staff/calendar`, `/staff/dashboard`, `/staff/field`, `/admin/services`, `/admin/integrations`, `/notices`, `/map`, `/staff/finance` and its statement/unmatched/bond/refund pages, and Tom's `/admin/prices` access. Opened `/staff/cases/<id>` for generic, venue_booking, equipment_hire, building, planning_certificate, road_issue and complaint, clicking every registered tab. Also tested the direct Money URL and Equipment → Money link.

Final result: **27 page visits; zero console errors, zero console warnings, zero unexpected HTTP failures.** Temporary browser contexts and servers were closed. This was a runtime/rendering audit; the complete money journeys were driven by curl rather than clicking every form in Chromium.

## Remaining known issues and limits

- `seed::scenarios::run` is still empty. Base seeding supplies services/personas/prices, but does not prepopulate the awaiting-payment cases named by the statement fixture. The smoke explicitly creates NSH-2026-000001 and NSH-2026-000002 through HTTP before importing that unchanged CSV. Importing it immediately into an otherwise empty demo will not yield the same match counts. Filling the general demo scenario catalogue remains separate work.
- Statement references/dates and several existing fixtures are deliberately FY2026-27/2026 examples. A later calendar-year demonstration needs updated fixtures; the smoke currently asserts the supplied 2026 references.
- The reschedule's $40 fee surplus remains customer credit as designed; the $200 refund is specifically the refundable portion of the bond. The credit is not silently treated as another completed refund.
- Vite retains the existing advisory about the approximately 518 kB main bundle. The build succeeds.
- Three existing Rust documentation examples remain ignored. No requested HTTP smoke flow is skipped.
- Payment, mail and records providers remain in-process mocks; this pass does not verify live vendor integrations.
