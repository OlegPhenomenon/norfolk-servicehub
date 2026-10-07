# S6C — Playwright browser journeys, accessibility and screenshots

Implemented section 3 of `.orchestra/specs/S6-scenarios-e2e.md` on `integration` in the main checkout. No commit was made. README changes are confined to its Testing section; the rest was compared with HEAD and is unchanged.

## E2E project and lifecycle

`e2e/` is an independent npm project with a lockfile, Playwright Test 1.63.0 and axe-core Playwright 4.13.0. Chromium revision 1243 was already cached in `~/Library/Caches/ms-playwright`; no browser installation was needed.

The global setup reuses `server/target/debug/servicehub` and `web/dist/index.html`. It runs `cargo build` or the web build only when the respective artifact is absent (and installs web dependencies only if needed for that fallback). After app edits, rebuild those artifacts explicitly. It allocates a free loopback port, seeds a new temporary DATA_DIR with the real `seed-demo` command, starts the ordinary `servicehub serve` binary in demo mode, and waits for `/api/health`. Demo reset and expiry are disabled for the disposable run. Checkout and webhook URLs use the same temporary server. Teardown stops the child, removes the temporary database/blobs, and retains the server log in `e2e/test-results/server.log`; setup failures also clean up.

One worker and zero test retries keep mutation stories independent of concurrent changes to the seeded catalogue. Each test receives fresh browser storage. Two-person stories use separate contexts. Every staff login reads the current TOTP from the visible Demo authenticator, fills the form, and verifies it. If another test already used that persona's code, the helper asserts the replay rejection and waits for the next visible code; it does not bypass MFA or obtain codes through a test API.

Run `make e2e`, or `cd e2e && npm ci && npx playwright test`. Failure screenshots, traces and full axe JSON are retained in the test results; the HTML report is in `e2e/playwright-report/`. Generated test artifacts are ignored, while README screenshots are tracked.

## Browser coverage

| Story | Browser actions and assertions |
| --- | --- |
| (a) Hall hire | Alexey signs in from `/demo`, starts the real hall form, selects a live available slot, completes declarations, uploads the required PDF, reviews and submits. Olga signs in with TOTP in another context, finds the reference in New, and advances intake. Alexey opens Money, follows Pay to the actual DemoPay HTML checkout and clicks **Pay with test card**. The UI waits for the webhook-confirmed receipt and the case header advances to **Confirming your booking** without reloading. Olga confirms through Booking; Alexey sees the confirmation history and downloads the actual confirmation PDF. |
| (b) Phone intake | Olga selects the road service and Phone channel, enters a fictional caller's contact details and coordinates, reviews and submits account-free assisted intake, opens the received reference, verifies the frozen answers/channel and advances triage to an inspection task. |
| (c) Offline field task | Jake opens an **open** seeded road inspection at 390×844, then `context.setOffline(true)` disconnects the actual browser. He completes both checklist items, saves a result and marks done. Four persisted device-save indicators are required before reconnecting. Read-only HTTP assertions prove the server task remains unchanged offline. Reconnecting automatically confirms all four commands, with exactly four new server updates, completed status and the saved result surviving reload. |
| (d) Service builder | Mark creates a new generic community-garden service, writes its description/outcome/policy, adds a required field, inspects workflow, validates sample answers through Preview and publishes. A second context registers a new fictional resident, finds the new service in the catalogue, fills the field, reviews and submits, and sees the reference and frozen answer. |

An additional browser regression delays the first **real** field-update request while the UI saves the remaining checklist/result/completion commands. It releases that request and verifies every command is sent exactly once without another network reconnect. No response or domain action is mocked.

## Application defects fixed

- **Stale case status after payment:** Money polling displayed **Payment confirmed**, while the case header and workflow still said **Payment is needed before we can continue**. Reproduced by a failing browser assertion before the fix. `web/src/features/finance/MoneyPanel.tsx` now invalidates the case detail when payment IDs or refund settlement states change. Story (a) is the regression for the visible header transition.
- **Offline save feedback disappeared:** the IndexedDB outbox query inherited TanStack Query's online-only network mode. Commands and optimistic task state were persisted, but **Saved on this device** indicators did not refresh after real network loss. `web/src/features/operations/useOutbox.ts` now runs that local query with `networkMode: 'always'`. Story (c) requires all offline acknowledgements before reconnecting.
- **Save during synchronization could remain queued:** the current synchronization batch took a snapshot; a new save reused its in-flight promise and was not included in another batch. `web/src/features/operations/outbox.ts` now records that another synchronization was requested and drains a follow-up batch. The delayed-request browser regression covers this race.

Field checklist changes are acknowledged after IndexedDB persistence. The tests click and wait for the checked/enabled state rather than assuming the controlled checkbox updates synchronously. Navigation also waits for the destination before interacting with controls that occur on both pages.

## Accessibility

axe-core runs on all requested routes: `/`, `/services`, `/services/rawson-hall-hire`, `/demo`, `/my`, `/my/cases/<id>`, `/staff`, `/staff/cases/<id>`, `/staff/field` at 390×844, `/staff/finance`, `/staff/dashboard`, `/admin/services`, `/notices`, and `/map`. Case IDs are discovered through visible seeded lists. Audits wait for content and fonts, rather than scanning a loading shell. An additional audit opens a seeded public notice and expands its published-plan preview.

All 15 audits pass with **zero serious or critical violations**. No axe rules, elements, map tiles or frames are excluded or suppressed. The full result includes lower-impact findings for inspection. These automated scans do not replace manual keyboard and assistive-technology testing.

## Screenshots and public plan preview

`cd e2e && npm run screenshots` uses a separate fresh seeded server to regenerate exactly:

`home.png`, `catalogue.png`, `hall-booking.png`, `staff-case.png`, `finance.png`, `calendar.png`, `field-mobile.png`, `dashboard.png`, `service-builder.png`, `exhibition.png`.

All are under `docs/screenshots/`, nine at 1440×900 and `field-mobile.png` at 390×844. The booking screenshot shows seeded booked hours/buffers and an available evening slot. Calendar navigation selects a week containing seeded hall and equipment reservations. Finance shows seeded outstanding invoices; the builder shows the existing dog-registration draft. Olga's workspace shows the workflow, guard message and action bar. Images come from the compiled app, with no Vite development overlays. Staff case, field mobile, hall widget and exhibition screenshots were opened and visually checked.

To show the redacted plan alongside its public notice, Documents now offers an expandable **Preview published plan**. The new public PNG endpoint authorizes the exhibition/item and renders only `published_blob_id`, sharing the existing bounded renderer. It never renders the private source for public users. Downloads retain their existing attachment behavior. The expanded Rust publication regression checks the preview's PNG type and burned black pixels, and rejects invalid pages/items; the original source and image-only published PDF checks remain in place.

## Verification

| Command/check | Result |
| --- | --- |
| `cd e2e && npx playwright test`, first final full run | **20 passed**, zero retries, approximately 1.5 minutes |
| `cd e2e && npx playwright test`, second consecutive final full run | **20 passed**, zero retries, approximately 1.2 minutes |
| `cd e2e && npm run screenshots` | **Passed**, all ten images generated; sizes checked from PNG headers |
| `cd server && cargo test` | **Passed: 131 tests** (122 unit/integration + 8 acceptance journeys + 1 seeded-history test), zero failures; three pre-existing doc examples ignored |
| `cd server && cargo clippy --all-targets -- -D warnings` | **Passed** |
| `cd web && npm run build && npm run lint && npm test` | **Passed**, 27 web tests, zero lint warnings |
| README outside Testing | Byte-for-byte unchanged |
| `git diff --check` | **Passed** |

No deployment, production database, seed scenario history or external vendor service was changed. The existing Vite advisory for the approximately 518 kB main bundle remains. Poppler is required by seeded exhibition publication and previews, as it is by the application. Providers are the application's actual in-process demos; the tests exercise their HTTP checkout/webhook cycle, not live vendor integrations.
