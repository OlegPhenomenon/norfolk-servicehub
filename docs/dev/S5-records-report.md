# S5 handoff — records

S5 is **not fully complete**. The records-owned behavior is implemented and verified, but five commands are blocked by missing transactional owner APIs. `contracts.rs` is an explicit bridge: its methods return 409 and the caller's transaction rolls back. It does not write another slice's tables. In particular, new complaint submission currently rolls back when automatic assignment reaches this bridge. Do not describe complaint submission/review, successful disposal, successful legacy import, or user deactivation as working until the bridges below are connected.

## Implemented endpoints and pages

| Area | API | Frontend |
|---|---|---|
| Dashboard | `GET /api/staff/dashboard`, `GET /api/staff/dashboard/metrics/{metric}` | `/staff/dashboard`, `/staff/dashboard/metrics/:metric` |
| Complaints | `GET /api/cases/{id}/complaint`, `POST …/subjects`, `POST …/request-review` | Confidential feedback panel for staff/applicant; subject exclusions and review links |
| Records | `GET /api/records/search`, `GET /api/records/disposal-candidates`, `GET /api/records/legal-holds`, `GET /api/records/cases/{id}`, `POST …/legal-hold`, `POST …/legal-hold/release`, `POST …/dispose` | `/staff/records`, Records case panel |
| Export | `GET /api/cases/{id}/export.zip` | Records panel download |
| Integrations | `GET /api/admin/integrations`, `POST …/{id}/retry`, `GET …/systems`, `POST …/systems/{code}`, `GET /api/admin/mock-records/{code}`, `GET /api/cases/{id}/integrations` | `/admin/integrations`, Records panel |
| Mock receiver | `POST /mock/records/{code}/api/records` with `X-Mock-Key` and `Idempotency-Key` | Remote contents and outage/response-loss controls on integrations page |
| Legacy | `GET/POST /api/admin/legacy-imports`, `GET …/{id}`, `POST …/{id}/import` | `/admin/legacy-import` |
| Organisation | `GET /api/my/organisations`, `POST …/{id}/invites`, `POST /api/my/invites/{token}/accept`, `POST …/{id}/members/{mid}/revoke` | `/my/organisation`, `/my/invites/:token` |
| Users | `GET/POST /api/admin/users`, `POST …/{id}/deactivate`, `GET/POST …/{id}/roles`, `POST …/{id}/roles/{grant}/revoke` | `/admin/users` |
| Authority | `GET/POST /api/staff/decision-authorities` | `/staff/authority` |
| Settings and deliveries | `GET/POST /api/admin/settings`, `GET /api/admin/deliveries`, `POST …/{id}/retry` | `/admin/settings`, `/admin/deliveries` |
| Retention rules | `GET/POST /api/admin/retention-rules` | `/admin/retention` |
| Backups | `GET /api/admin/backups`; existing CLI calls `records::backup::{backup,restore_check}` | `/admin/backups` |

All list/search/count queries concerning cases use `authz::case_scope_sql`; individual operations use `require_case`/`require_staff_case`. Sysadmin diagnostics omit case payloads and links unless a separate role grants access. Case exports include metadata, the frozen submission, messages, internal notes, assignments, decisions/evidence, money summary, scoped links, integrations, live document files, and a README. ZIP entries use safe filenames and standard ZIP32 storage.

Metrics have one predicate shared by counts and drill-downs. Current open/waiting/unassigned/overdue/due-soon/reopened counts are snapshots; received uses submitted date and terminal outcomes use closed date in the inclusive Norfolk reporting period. Reopened cases are counted separately and excluded from completed counts and completion medians. Workload and service breakdown numbers, including medians, link to the same filtered lists. Due-soon uses the next three Norfolk business days.

Integration hooks atomically queue stable operations (`record.case_closed:<id>`, `document.decision:<id>`, `payment.receipt:<id>`). The existing durable jobs worker handles retry backoff. Delivery diagnostics remain failed/dead until recovery; the receiver's first response can be delayed ten seconds against the sender's five-second timeout. Retry receives the saved reference. Manual retries reset the existing job, avoiding duplicate senders. Rules, systems, mock switches and notification-address seed data are idempotent and clearly marked as demonstration settings/policies.

## Validation

Final results: `cargo test` passed all 42 tests (3 documentation examples ignored); `cargo clippy -- -D warnings` passed; `npm run build` passed; `npm run lint` passed with zero warnings; `git diff --check` passed.

Records tests cover metric count/list agreement and cancelled/reopened separation; subject denial through the triage endpoint plus scoped reads/search/count/export; outage and manual retry; first-response loss with exactly one remote record; backup/restore and tampered-file rejection; email-bound invite acceptance and immediate revocation; legal-hold disposal refusal; manager-only/non-self authority grants; strict quoted CSV parsing and the 15-row sample's error/exact/possible duplicates.

Tests seed other modules' rows directly and do not require their current implementations. Successful review/import/disposal/deactivation tests await the owner bridges. The revoked-member document-download 404 must additionally be verified against S2's real download route after merge; S5 verifies immediate loss of the shared case authorization used by that route. There was no browser interaction/screenshot verification.

## Deviations

- `VACUUM INTO` is illegal inside a SQLite transaction. Backup holds `BEGIN IMMEDIATE` on the source pool while a dedicated read-only connection runs VACUUM and copies files. That deliberate backup exception to the normal no-second-connection rule is tested. Referenced blobs are discovered from schema foreign keys; disposed-only document references are excluded while immutable metadata survives.
- Notification retry creates a new outbound row through `notify::send`, preserving the original failed row. Exhausted queued jobs are projected as failed in the delivery log.
- Exact CSV sources are always skipped; one explicit boolean decides whether possible duplicates are skipped. The upload API accepts JSON `{filename,csv}` because the platform axum build has no multipart support. The frontend reads a selected CSV file and sends this envelope; storage still validates/stages/registers the original CSV.
- Optional `expected_revision` is accepted for case commands whose task payload did not specify it. The UI sends it for subjects and legal holds; supplied stale values fail atomically.
- `records.backup` self-schedules daily and demo seeding queues its first execution. Fresh unseeded serve mode needs the platform scheduler hook below. Failed snapshot attempts use separate directories and do not stop the next day's schedule.
- Operational docs and the CSV sample were added at the explicitly requested paths. No shared schema, platform source, other feature directories, or dependency manifests were edited. No commit was made.

## Requests for other slices

S1/platform should expose these transaction-taking APIs and replace the corresponding `records/contracts.rs` bridges with calls:

```rust
cases::api::assign_owner(
    tx: &mut SqliteConnection, actor: &Actor, case_id: i64,
    user_id: i64, reason: &str,
) -> AppResult<()>;

cases::api::create_review(
    tx: &mut SqliteConnection, actor: &Actor, original: &CaseRow,
    reason: &str,
) -> AppResult<CaseRow>;

cases::core::set_import_dates(
    tx: &mut SqliteConnection, case_id: i64, submitted_at: &str,
    closed_at: Option<&str>,
) -> AppResult<()>;
```

`assign_owner` must end any prior owner before creating a new one. `create_review` must create the submitted complaint, allocate its number, copy the frozen definition/submission, start triage, and create a `review_of` link, but must not invoke automatic assignment before S5 copies exclusions. S5 then copies subjects/denials, selects a different eligible complaints officer (manager fallback), assigns and sends a generic notification. `set_import_dates` must preserve the historical timestamps rather than today's creation time.

S2 should provide:

```rust
documents::api::dispose_case_files(
    tx: &mut SqliteConnection, case_id: i64,
) -> AppResult<Vec<String>>; // only orphan blob hashes eligible for removal
```

It must set `documents.disposed_at`, preserve metadata and exact version/evidence IDs, and retain any blob with another live consumer. Coordinate file cleanup through storage to avoid racing a new reference. S5 keeps disposal events, history, hold checks and audit. S2's canonical `documents.letter_issued:complaint_response` guard remains the workflow guard; S5 does not duplicate the generic hook dispatcher.

Platform should provide:

```rust
auth::users::deactivate_user(tx: &mut SqliteConnection, user_id: i64) -> AppResult<()>;
```

It should deactivate the account and revoke sessions atomically. Add `records::schedule_daily(&mut tx,state).await?` in `jobs::schedule_tick` so unseeded installations queue backups. The records dispatch prefix is already present; no jobs.rs edit was needed or made.

S1/platform must ensure **all** outbound notifications for confidential cases are generic, preferably at the shared `notify::send` boundary. The parallel S1 `post_staff_message` currently passes the message body through as the email body. S5's own notices already contain only `There is an update on your feedback NSH-…`; it cannot safely fix another slice's notification path.

After wiring these APIs, run successful complaint submission/review (different owner and copied exclusions), re-import idempotency, shared-blob disposal with metadata preservation, deactivation/session loss, and actual document-download revocation tests. Replace the bridge errors before declaring S5 complete.
