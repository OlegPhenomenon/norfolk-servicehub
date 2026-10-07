# Norfolk ServiceHub — Architecture

A demonstration of a single web application where Norfolk Island residents and businesses request council services and council staff carry each request through to a result: an answer, an issued document, a completed job or a closed hire.

> Demo scope: all people, cases and documents are fictional. Payment provider, email/SMS gateway, records systems
> (Content Manager, Civica Altitude) and the AI helper are **mock services built into the same binary**, but the
> technical cycle around them (hosted checkout, signed webhooks, idempotency, retries, outbox, outage recovery) is real.

## 1. Shape

```
┌──────────────────────── one process: `servicehub serve` ────────────────────────┐
│  axum HTTP (PORT)                                                                │
│   /api/**        JSON API (resident, staff, admin, public)                       │
│   /mock/**       mock external services (DemoPay checkout + API, DemoMail,       │
│                  records systems, AI helper) — clearly branded "test mode"       │
│   /**            React SPA (web/dist) with index.html fallback                   │
│  background worker (tokio task): jobs table → notify / integration / deadlines   │
│  SQLite (WAL) DATA_DIR/servicehub.db      blobs DATA_DIR/blobs/ab/abcdef…        │
└──────────────────────────────────────────────────────────────────────────────────┘
```

- **Backend:** Rust, single crate `server/` → binary `servicehub`. axum 0.8, tokio, sqlx 0.8 (SQLite), chrono + chrono-tz, argon2, totp-rs, printpdf, reqwest (rustls), clap.
- **Frontend:** `web/` React 19 + TypeScript + Vite, React Router, TanStack Query, Tailwind CSS 4, Leaflet.
- **Database:** SQLite, WAL, `foreign_keys=ON`, `busy_timeout=5000`, `synchronous=FULL`. Schema: `server/migrations/`.
- **Deployment:** one Docker image (binary + `web/dist` + poppler-utils), behind the host's reverse proxy.

### CLI

`servicehub serve | migrate | seed-demo | reset-demo | backup <dir> | restore-check <backup-dir>`

### Configuration (environment; see `.env.example`, no secrets in git)

| Var | Default | Meaning |
|---|---|---|
| `PORT` | `8080` | HTTP port |
| `DATA_DIR` | `./data` | SQLite file + blobs + backups |
| `PUBLIC_BASE_URL` | `http://localhost:8080` | used in links, checkout return URLs |
| `INTERNAL_BASE_URL` | `http://127.0.0.1:$PORT` | server-to-server calls to `/mock/**` and webhooks |
| `WEB_DIST` | `../web/dist` | static SPA |
| `DEMO_MODE` | `false` | persona login, demo authenticator, scheduled reset, demo banner |
| `DEMO_RESET_HOURS` | `6` | reset interval in demo mode (0 = never) |
| `COOKIE_SECURE` | `true` | set `false` for plain-http local dev |
| `WEBHOOK_SECRET` | random per start | HMAC secret shared by DemoPay and the webhook handler |
| `MOCK_API_KEY` | random per start | header `X-Mock-Key` required on server-to-server `/mock/**` APIs |

## 2. Backend module map (`server/src/`)

Ownership matters: a module **writes only its own tables** (see section comments in `0001_init.sql`). Reading other modules' tables in SQL for display/reporting is allowed. Cross-module writes go through the owner's `api.rs` functions, which take a caller-owned transaction so composite commands stay atomic.

| Path | Owner | Contents |
|---|---|---|
| `main.rs`, `config.rs`, `state.rs` | platform | CLI, env config, `AppState` |
| `error.rs` | platform | `AppError`, `AppResult<T>`, JSON error envelope |
| `db.rs` | platform | pool setup, `write_tx()` (BEGIN IMMEDIATE), helpers |
| `clock.rs`, `time.rs` | platform | `Clock` trait (real + fixed for tests), RFC 3339 helpers, `Pacific/Norfolk` local dates |
| `calendar.rs` | platform | business-day arithmetic over `holidays` |
| `auth/` | platform | sessions, passwords, TOTP, `Actor` extractor, demo persona login |
| `authz.rs` | platform | roles, `case_access()`, `case_scope_sql()` — **the only place access rules live** |
| `audit.rs` | platform | `audit::record(tx, …)` |
| `storage.rs` | platform | blob put/get, upload validation (size, magic bytes, allow-list) |
| `jobs.rs` | platform | durable queue, worker loop, `jobs::enqueue(tx, …)`, handler dispatch by `kind` prefix |
| `notify.rs` | platform | `notify::send(tx, …)` → in-app row + outbound email/SMS job via DemoMail |
| `idempotency.rs` | platform | `idempotent(tx, actor, scope, key, request_hash, || …)` |
| `pdf.rs` | platform | `pdf::simple_document(title, meta, sections) -> Vec<u8>` (shared by all modules) |
| `cases/core.rs` | platform | case primitives: `create_case`, `append_event`, `load_case`, `reindex_search`, number allocation |
| `hooks.rs` | platform (dispatch) | per-module hook dispatch: `validate_field`, `on_submit`, `on_step_entered`, `step_guard`, `pricing_lines` |
| `services/` | services | catalog, definitions, validation, builder, bulk import, mock-AI suggestion |
| `cases/` (other files) | services | drafts, submission, workflow engine, assignment, messages, notes, links, search, assisted intake |
| `deadlines/` | services | deadline engine (business/calendar days, pauses, breaches) |
| `documents/` | documents | documents & versions, comments, decisions & templates, building projects, planning certificates, exhibitions & redaction |
| `operations/` | operations | resources, bookings & calendar, equipment hire & usage, field tasks & offline sync, road issues |
| `finance/` | finance | price lists, invoices, DemoPay checkout & webhooks, bank statement import & matching, allocations, deposits, refunds, ledger |
| `records/` | records | complaints, manager dashboard, retention & legal hold, integrations outbox, legacy import, case export, admin (users, settings, delivery log, backups) |
| `mock/` | each owner | `/mock/**` routers: `mail.rs` (platform), `pay.rs` (finance), `records.rs` (records), `ai.rs` (services) |
| `seed/` | each owner | `seed/mod.rs` orchestrates; each module exposes `seed(tx)` |

Each module exposes `pub fn routes() -> Router<AppState>` merged in `main.rs`, and optionally `pub mod api` (cross-module functions), `pub async fn handle_job(...)` and `pub fn seed(...)`.

## 3. Conventions

- **API:** JSON under `/api`. Resident and staff use the same endpoints; responses are projected by `authz`.
- **Errors:** `{"error": {"code": "...", "message": "Human sentence", "fields": {"field": "message"}}}` with codes `unauthorized` 401, `mfa_required` 401, `forbidden` 403, `not_found` 404, `conflict` 409, `stale_revision` 409, `idempotency_mismatch` 409, `validation` 422, `rate_limited` 429, `internal` 500. Never leak existence of a case the actor cannot see: return `not_found`.
- **Auth:** opaque session cookie `nsh_session` (HttpOnly, SameSite=Lax, Secure unless `COOKIE_SECURE=false`). Every non-GET `/api` request must send `X-CSRF-Token` (value from `GET /api/me`), except `/api/webhooks/**` and unauthenticated public endpoints (rate-limited instead).
- **Staff 2FA:** staff sessions have `mfa_passed=0` until TOTP verified; staff endpoints return `mfa_required` until then.
- **Concurrency:** staff commands on cases/bookings/tasks carry `expected_revision`; mismatch → `stale_revision`. All write transactions use `db::write_tx()` (`BEGIN IMMEDIATE`).
- **Idempotency:** case submission, payment confirmation, offline task sync and refund requests accept `Idempotency-Key` header; same key + same body returns the stored response, different body → `idempotency_mismatch`.
- **Money:** integer cents, AUD; `quantity_milli` for fractional quantities; rounding half-up at line level only.
- **Time:** stored UTC; displayed and reasoned about in `Pacific/Norfolk`.
- **Audit:** every state-changing command writes `audit_log` and, when case-related, a `case_events` row with a plain-English `summary`.
- **AI:** optional helper only. It suggests service-definition drafts; it never computes amounts, deadlines or decisions. With AI disabled everything works.

## 4. Roles and access (implemented once in `authz.rs`)

| Role | Sees | Can |
|---|---|---|
| resident (user kind) | own cases; org cases while membership active; cases where active representative | draft, submit, reply, upload new versions, pay, withdraw |
| `intake` | all non-confidential cases | assisted intake, triage, assign, message, request info |
| `specialist` | non-confidential cases of services in scope (NULL scope = all) | assess, comment on documents, prepare decisions, exhibitions |
| `finance` | money views of non-confidential cases | statements, matching, refunds, deposit decisions, invoices |
| `field_worker` | only tasks assigned to them (task projection, no applicant documents) | update tasks, record usage |
| `manager` | all non-confidential cases, dashboard | reassign, escalate, grant decision authority |
| `complaints_officer` | confidential complaint cases | handle complaints and reviews |
| `sysadmin` | configuration only — **no case content** unless they also hold another role | users, services, prices, settings, integrations, backups |

Rules evaluated by `authz::case_access(tx, actor, case_id) -> CaseAccess`:
1. A row in `case_access_denials` for (case, actor) → **no access**, overriding everything.
2. Applicant access: `applicant_user_id = actor`, or active membership in `applicant_org_id`, or active `case_representatives` row. Projection: `Applicant` (no internal notes, no staff-visibility events/documents/comments).
3. `confidential = 1` cases: only `complaints_officer`, active assignees, and managers (unless denied by rule 1).
4. Otherwise staff per table above. Field workers get `TaskOnly`.
`authz::case_scope_sql(actor)` returns a SQL predicate over alias `c` implementing **the same rules** for listings, search, counts, dashboard drill-downs and exports. Tests assert both functions agree.

Decision authority: `decision_authorities` rows, granted by a manager to someone else; `sysadmin` cannot grant or hold it implicitly.

## 5. Service definition (`service_versions.definition_json`)

```jsonc
{
  "summary": "Hire one or both rooms of Rawson Hall for an event.",
  "outcome": "A confirmed booking with date, rooms and conditions (PDF).",
  "who_can_apply": "Anyone; businesses can apply on behalf of their organisation.",
  "price_note": "Session fee per room plus a refundable bond. Illustrative demo prices.",
  "keywords": ["hall", "venue", "party", "wedding", "function", "room hire"],
  "fields": [
    { "key": "event_name", "type": "text", "label": "Event name", "required": true, "max_length": 120 },
    { "key": "slot", "type": "booking_slot", "label": "Date, time and space", "required": true,
      "venue": "Rawson Hall" },
    { "key": "alcohol", "type": "select", "label": "Will alcohol be served?", "required": true,
      "options": [ { "value": "no", "label": "No" }, { "value": "yes", "label": "Yes" } ] },
    { "key": "liquor_permit", "type": "text", "label": "Liquor permit number", "required": true,
      "show_if": { "field": "alcohol", "equals": "yes" } }
  ],
  "documents": [
    { "key": "insurance", "label": "Public liability insurance (if applicable)", "required": false,
      "accept": ["application/pdf", "image/png", "image/jpeg"], "public_candidate": false }
  ],
  "workflow": { "steps": [
    { "key": "intake",   "kind": "review",   "role": "intake",  "label": "Check request",
      "applicant_label": "We are checking your request." },
    { "key": "payment",  "kind": "payment",  "role": "finance", "label": "Fees and bond paid",
      "applicant_label": "Payment required: hire fee and bond." },
    { "key": "confirm",  "kind": "module",   "role": "intake",  "label": "Confirm booking",
      "applicant_label": "Payment received — confirming your booking." },
    { "key": "prep",     "kind": "task",     "role": "field_worker", "task_kind": "venue_prep",
      "label": "Prepare hall", "applicant_label": "Your booking is confirmed." },
    { "key": "inspect",  "kind": "task",     "role": "field_worker", "task_kind": "venue_inspection",
      "label": "Post-event inspection", "applicant_label": "Event finished — hall inspection." },
    { "key": "bond",     "kind": "module",   "role": "finance", "label": "Bond decision and refund",
      "applicant_label": "We are processing your bond." },
    { "key": "done",     "kind": "complete", "label": "Closed", "applicant_label": "Hire completed." }
  ]},
  "deadlines": [
    { "kind": "completeness", "label": "Initial check", "days": 3, "basis": "business",
      "starts": "submitted", "stops": "step:payment", "pausable": false },
    { "kind": "decision", "label": "Decision", "days": 20, "basis": "business",
      "starts": "step:assessment", "stops": "decision_issued", "pausable": true, "max_pause_days": 30 }
  ],
  "pricing": [ { "item": "PLANNING_CERT", "quantity": 1 } ],
  "decision_types": ["planning_certificate"]
}
```

Field types: `text`, `textarea`, `number`, `date`, `time`, `email`, `phone`, `select`, `multiselect`, `checkbox`, `property_ref` (Portion/Lot reference), `location` (map point + description), `booking_slot` (operations), `equipment_request` (operations). Conditional display: `show_if {field, equals}`. **No arbitrary expressions or code in definitions.**

Workflow step kinds and their guards (checked by the workflow engine when staff press *Advance*):

| kind | guard to leave the step |
|---|---|
| `review` | staff with the step role decides (advance / request info / refuse) |
| `payment` | `finance::api::case_settled(tx, case_id)` — every issued invoice line fully allocated with confirmed money |
| `decision` | `documents::api::issued_decisions(tx, case_id)` contains every `decision_types` entry required by the step (`step.decision_type`); a refusal routes the case to `refused` |
| `task` | all tasks created for this step are `done` (task created on entry via `operations::api::create_step_task`) |
| `module` | `hooks::step_guard(module, step_key)` returns no blocking reason |
| `complete` | terminal; case → `completed` |

Entering a step calls `hooks::on_step_entered` (e.g. finance issues the invoice on entering `payment` using `hooks::pricing_lines`).

Every submitted case stores the full definition snapshot (`submissions.definition_snapshot_json`); later edits to the service never change existing cases.

## 6. Cross-module Rust API (stable signatures)

```rust
// platform
pub struct Actor { pub user_id: i64, pub kind: UserKind, pub roles: Vec<RoleGrant>, pub display_name: String, pub mfa_passed: bool }
pub enum CaseAccess { None, Applicant, Staff { can_manage: bool }, TaskOnly }
authz::case_access(tx, &Actor, case_id) -> AppResult<CaseAccess>
authz::require_case(tx, &Actor, case_id) -> AppResult<(CaseRow, CaseAccess)>   // not_found if None
authz::case_scope_sql(&Actor) -> ScopeSql { sql: String, binds: Vec<SqlValue> }
cases::core::create_case(tx, NewCase) -> AppResult<CaseRow>
cases::core::append_event(tx, case_id, actor: Option<i64>, kind, Visibility, summary, data: Value) -> AppResult<i64>
cases::core::load_case(tx, case_id) -> AppResult<CaseRow>
cases::core::bump_revision(tx, case_id, expected: Option<i64>) -> AppResult<i64>
cases::core::reindex_search(tx, case_id) -> AppResult<()>
audit::record(tx, actor: Option<i64>, action, entity_type, entity_id, details: Value) -> AppResult<()>
notify::send(tx, Notice { user_id, email, phone, case_id, subject, body, link }) -> AppResult<()>
jobs::enqueue(tx, kind, payload: Value, idempotency_key: Option<String>, run_after) -> AppResult<()>
storage::put(state, bytes, original_name, actor) -> AppResult<BlobRow>   // validates
storage::read(state, blob_id) -> AppResult<(BlobRow, Vec<u8>)>
calendar::add_business_days(tx, start_local_date, n) -> AppResult<NaiveDate>
pdf::simple_document(title, meta: &[(&str, String)], sections: &[(&str, String)]) -> Vec<u8>

// services (cases workflow)
cases::workflow::advance(tx, &Actor, case_id, expected_revision, note) -> AppResult<CaseRow>
cases::workflow::close(tx, &Actor, case_id, outcome, reason) -> AppResult<()>
deadlines::api::pause_for_applicant(tx, case_id, message_id, reason) -> AppResult<()>
deadlines::api::resume(tx, case_id, why) -> AppResult<()>

// documents
documents::api::attach_generated(tx, case_id, category, title, visibility, pdf_bytes, actor) -> AppResult<DocumentId>
documents::api::issued_decisions(tx, case_id) -> AppResult<Vec<DecisionSummary>>

// operations
operations::api::create_step_task(tx, &CaseRow, &StepDef, actor) -> AppResult<i64>
operations::api::step_tasks_done(tx, case_id, step_key) -> AppResult<bool>

// finance
finance::api::quote(tx, item_code, quantity_milli, pricing_date) -> AppResult<QuoteLine>
finance::api::issue_invoice(tx, &Actor, case_id, kind, lines: Vec<QuoteLine>, basis_note) -> AppResult<i64>
finance::api::case_settled(tx, case_id) -> AppResult<bool>
finance::api::case_money_summary(tx, case_id) -> AppResult<MoneySummary>

// hooks.rs dispatch (module = services.module)
hooks::validate_field(tx, module, &FieldDef, &Value) -> AppResult<Option<String>>
hooks::on_submit(tx, &Actor, &CaseRow, answers: &Value) -> AppResult<()>
hooks::on_step_entered(tx, &Actor, &CaseRow, &StepDef) -> AppResult<()>
hooks::step_guard(tx, &CaseRow, &StepDef) -> AppResult<Option<String>>   // Some(reason) blocks
hooks::pricing_lines(tx, &CaseRow) -> AppResult<Vec<QuoteLine>>
```

## 7. Frontend (`web/src/`)

```
main.tsx, App.tsx, routes.tsx            shell + route assembly (platform)
api/client.ts                            fetch wrapper: CSRF header, error envelope → ApiError, Idempotency-Key helper
auth/                                    useMe(), persona picker (/demo), login, TOTP step
ui/                                      design system: Button, Field, Select, Card, Badge, StatusPill, Table, Tabs,
                                         Timeline, Money, DateTime, Dialog, Toast, EmptyState, PageHeader, Alert
layout/                                  PublicLayout, ResidentLayout, StaffLayout (sidebar), AdminLayout, DemoBanner
features/<module>/                       each slice owns its folder:
   routes.tsx       export residentRoutes, staffRoutes, adminRoutes, publicRoutes: RouteObject[]
   nav.ts           export staffNav, adminNav: NavItem[]
   casePanels.tsx   export casePanels: CasePanel[]   (tabs on the case page)
   fieldTypes.tsx   export fieldTypes: Record<string, FieldComponent>   (custom form widgets)
registry.ts                              imports the four exports from every feature and concatenates them
```

Areas: `/` public (service catalog, exhibitions, public road-issue map), `/my` resident/business, `/staff` workspace, `/admin` configuration, `/demo` persona picker. English UI, keyboard accessible, 200 % zoom safe, mobile-first for `/staff/field`.

## 8. Demo mode

- `/demo` lists personas: **Alexey Turner** (resident), **Island Builders Pty Ltd — Ben Carter** (business), **Olga Novak** (Customer Care intake), **Priya Nair** (Planning & Building specialist, holds decision authority), **Tom Becker** (Finance), **Jake Rowe** (Works Depot field worker), **Helen Ford** (Manager), **Ruth Adams** (Complaints officer), **Mark Ellis** (Systems administrator). All fictional.
- Staff personas must pass TOTP; the demo page shows a **Demo authenticator** with the live code (demo mode only). Self-hosted mode disables persona login and the authenticator endpoint entirely.
- Data resets every `DEMO_RESET_HOURS`; the banner shows the next reset. Seeded history is created through the same domain functions as live actions.
- DemoPay (`/mock/pay`), DemoMail outbox (`/mock/mail`) and records-system consoles are visible so viewers can see what "would have been sent".
