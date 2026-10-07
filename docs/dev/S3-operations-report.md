# S3 operations handoff

Implemented in the S3-owned directories and `server/migrations/0301_operations.sql`. No commit was created.

## Backend endpoints and domain behaviour

- Public `GET /api/public/venues/rawson-hall/availability`: three units backed by two atomic rooms, buffered active occupancy intervals, public labels only. Requested bookings do not block availability.
- Staff `GET /api/staff/calendar?from=&to=&resource=`: resources, confirmed/equipment/maintenance intervals and separate requests. Case labels/links require case access; denied cases still appear as anonymous unavailable intervals.
- Admin `GET /api/admin/resources`, `PATCH /api/admin/resources/{id}`, `POST /api/admin/maintenance`. Manager/sysadmin configuration access, bounded buffers, overlap checks and audits. Existing occupancies keep their allocated buffer times.
- `GET /api/cases/{id}/booking`, `POST .../booking/confirm`, `POST .../booking/reschedule/preview`, `POST .../booking/reschedule`, `POST .../booking/cancel`. Confirmation checks settlement and allocates all rooms inside one immediate write transaction. Rescheduling preserves old occupancy on any failure, retains revision history, calls finance repricing when item/amount changes, refreshes open venue tasks, regenerates confirmation and notifies the applicant and assignees. Cancellation releases occupancy and alerts finance for its refund decision.
- `GET /api/cases/{id}/booking/confirmation/{version}`: authorized download of a specific immutable confirmation PDF. Generated documents use the documents API; download uses existing storage.
- `GET /api/field/tasks`, `GET /api/field/tasks/{id}`, `POST .../{id}/updates`, `GET .../{id}/photos/{update_id}`, `GET /api/cases/{id}/tasks`, `POST /api/tasks/{id}/assign`, `POST .../{id}/cancel`. Worker projection deliberately excludes applicant contact data, documents and internal case notes. Commands use platform idempotency, persist offline timestamps, audit/events, optimistic revisions and current-state conflicts. Completion requires checklist + result, and an equipment job card when applicable, then invokes workflow auto-advance.
- `POST /api/cases/{id}/equipment/schedule`, `POST /api/field/tasks/{id}/usage`, `POST /api/cases/{id}/equipment/usage/{uid}/approve`, `GET /api/cases/{id}/equipment`, `GET /api/staff/operations/resources`. Operator/resource/time assignments are checked; actual billing uses exact integer minutes. One immutable job card per hire; finance approval creates the final invoice with calculation inputs and explanatory basis note. Expenses are agreed pass-through amounts. Estimates are issued only through finance's estimate API.
- `GET /api/public/road-issues`: exactly id/category/location/status_text/reported_on; no reporter names, photos or free text. `GET/POST /api/cases/{id}/road-response` supplies staff editing revision and issues an applicant-visible letter through documents.
- Hooks implement canonical field validation, submission rows, venue pricing at Norfolk booking date, and operations handler guards. Generic payment/task entry and guards remain in platform hooks.
- `operations::api::{create_step_task,step_tasks_done}` implemented using step-run IDs, operational-only instructions and default Jake assignment. Repeated creation of the same step task is idempotent; empty task runs do not pass the guard.

## Frontend

- Custom booking-slot, equipment-request and location widgets; live room availability strip, dates/times converted using Pacific/Norfolk and accessible coordinate alternative to map clicking.
- Booking, Tasks, Equipment and staff Road response case panels. Revision history, confirmation downloads, settlement/availability reasons, reschedule fee preview, staff assignment and scheduling, estimate versus actual charges.
- `/staff/calendar`: seven-day resource rows, whole venue on both room rows, requests, confirmed reservations, maintenance and shaded preparation/cleanup times. Maintenance form for managers.
- `/staff/field` and `/staff/field/:id`: worker list/detail, checklist, notes/photos/results/job cards, large controls, device outbox status, automatic online/load sync and simulate-offline toggle. IndexedDB stores commands and the optimistic task snapshot atomically, scoped to user. Revision conflicts stop subsequent commands for that task and show the server version; keeping that version/discarding queued changes requires a deliberate UI choice.
- Public `/map`: Leaflet + OpenStreetMap attribution, status-coloured pins, accessible report list. `/admin/resources`: resource buffers/active settings and maintenance.
- Registered staff/admin/resident nav. `publicNav` is exported for platform integration but the current platform registry does not consume it.

## Validation

- `cargo test`: PASS, 37 tests including five S3 tests.
- `cargo clippy -- -D warnings`: PASS.
- `npm run build`: PASS. Vite reports an advisory main-chunk size warning.
- `npm run lint`: PASS, zero warnings.
- `npm run test`: PASS, 12 tests including three S3 tests.
- `git diff --check`: PASS.
- S3 Rust invariants: two spawned Tokio tasks confirming whole venue versus Main hall on a file-backed, multi-connection SQLite DB yield exactly one success; failed reschedule preserves original occupancy/history; half-open buffer boundaries; Norfolk midnight/hours; exact usage/downtime; task privacy/idempotency/stale state/step-run isolation; public road projection privacy; repeated seed remains idempotent.
- Frontend tests: summer/winter Norfolk conversions, midnight date, sequential offline optimistic revisions without mutating the original, job-card commands do not invent task revisions.
- Live Chrome check with a temporary demo DB: public map and worker task UI rendered; worker navigation only has Field tasks; queued note wrote zero server updates while simulated offline, then exactly one after reconnect with “Confirmed by server ✓”; queued stale result showed server revision/result and did not overwrite it. Field task detail visually inspected at 360 px. Test browser viewport was restored.

Finance/documents/workflow remain the provided stubs in this isolated worktree. Operations tests seed their own fixtures and do not depend on those implementations. Full payment/PDF/advance journeys must be exercised after merging the owner slices.

## Deviations and decisions

- Research explicitly says hall capacities are not published. Capacity remains NULL; the UI says to confirm capacity with Council.
- Closest published plant names: EXCAVATOR / EQUIP_EXCAVATOR_HOUR = Bobcat; BACKHOE / EQUIP_BACKHOE_HOUR = Volvo Loader; TIPPER / EQUIP_TIPPER_HOUR = Hino Truck; ROLLER / EQUIP_ROLLER_HOUR = Cat Steel Drum Roller 8T. Fleet instances and maintenance are fictional.
- Leaflet 1.9.4 is vendored with its licence inside the owned frontend feature; no shared package/lockfile edits. Tiny native IndexedDB wrapper avoids another dependency.
- Field photos use JSON `{photo:{name,base64}}`; frontend caps files at 8 MB to fit the platform's 12 MB request limit. The server validates detected file content via platform storage.
- Task stale-state details use `error.fields.current_state` (serialized TaskOnly JSON) to preserve the existing platform error envelope. Replays return original result plus `status: already_applied`; mismatched payloads return 409.
- End midnight is exclusive and therefore does not count the next local day for hall charging. Prices always display the prescribed demo-copy label.
- Equipment schedule/cancel booking accept optional expected revision for compatibility with the specified payloads; the frontend always sends it. Task status/result/checklist commands require it. Job cards and approvals are immutable/idempotent.
- Changes add schedule/operator, command and invoice provenance columns only to operations tables; platform migrations are untouched.

## Requests for other slices

1. S1: include the Rawson Hall conditions in the service definition summary/help: music stops by 10 pm unless agreed; keys/property back by noon on the next business day; $20M public liability or agreed casual-hirer cover; more than seven days' meeting cancellation notice, thirty days for weddings/concerts/stage shows/balls. These already appear in the widget, unit description and generated confirmation. Venue task instructions recognize `event_name` or `event_title`, and `setup_notes` or `other_instructions`; keep those catalogue answer keys aligned.
2. S4: align price names/rates with the fleet mapping above (Bobcat $135/hr; Volvo Loader $230/hr; Hino Truck $110/hr; Cat Steel Drum Roller 8T $211/hr). Equipment usage approval already issues the final invoice before entering the payment step; `ensure_invoice_for_step` must reuse it. The Equipment panel links to `?tab=finance.money`; please use that panel key or adjust the link during integration.
3. Platform/orchestrator: connect exported `operations.publicNav` to the public header (its current links are fixed), so “Road issues map” appears publicly. Routes and resident nav are already registered.
4. No new cross-module Rust function is required: all calls use existing agreed APIs. Confirm PDF/settlement/repricing, issue-letter and task-done auto-advance integration remain merge-time checks against the owner implementations.
