# Integration pass 1 — non-finance

Completed on branch `integration`, 7 October 2026. No commit was made. Finance source files, the payment mock, finance frontend files, and `deploy/` were not modified.

## Cross-slice wiring

| Request | Result |
| --- | --- |
| S5 owner assignment | Added `cases::api::assign_owner`, using the existing assignment primitive to end previous owners and validate the new owner's access in the caller's transaction. HTTP authorization remains at the assignment route. S5 now calls the real API. |
| S5 independent complaint review | Added `cases::api::create_review`. It creates a numbered complaint with the original frozen definition, answers, submission document references, triage run, deadlines, and `review_of` link. It does not run submission hooks or assign an owner before S5 copies exclusions. Smoke confirms a different review owner and continued exclusion of Olga. |
| Historical imports | Added `cases::core::set_import_dates`, with timestamp validation and historical created/submitted/closed/updated dates. S5 calls it directly. Import replay and exact duplicate detection pass over HTTP. |
| Bridge cleanup | Replaced all S5 bridge calls with case, document, and auth owner APIs; deleted `records/contracts.rs`. |
| Document disposal | Added `documents::api::dispose_case_files`. Disposal marks documents disposed while retaining document/version IDs, blob metadata, submission references, and decision evidence. S5 commits its disposal event and invokes `storage::gc`; it no longer removes files directly. |
| Shared blob safety | GC preserves bytes referenced by live documents, other blob consumers, or another undisposed case's frozen submission/decision evidence. Backup inventory follows the same cross-case evidence rule. GC holds the SQLite writer lock through deletion, serializes local staging, and registration restores staged bytes if cleanup occurred before registration. The existing ten-minute staging grace remains: newly staged disposed bytes can be collected by a later GC. |
| User deactivation | Added `auth::users::deactivate_user`: deactivation and deletion of all sessions occur in the caller's transaction. S5 uses it. Tests cover rollback and commit; smoke confirms immediate invalidation of the old session. |
| Daily records jobs | `jobs::schedule_tick` calls `records::schedule_daily`. Tests cover an unseeded database and job deduplication. |
| Confidential notifications | `notify::send` checks confidentiality at the delivery boundary and substitutes a generic subject and body for email, SMS, and in-app notifications: `There is an update on your feedback NSH-… — sign in to read it`. Sensitive complaint message content is absent from the smoke's outbound notification assertions. |
| Replacement drawing requests | Document v2 upload resolves linked replacement comments and their required-action messages atomically. The action disappears and deadlines resume only when no required request remains. A text reply does not satisfy an outstanding replacement request. The deadline reason uses the existing `applicant_responded` database value. |
| S2/S3 answer contracts | Hall uses `event_name` and `setup_notes`; modification uses `original_approval` with field type `decision_ref`; commencement/completion notices use `project_reference`; certificate has `sections`. Updated notice definition validation to recognize the canonical key. HTTP smoke verifies modification and both notice forms link to the existing building project. |
| Rawson Hall conditions | Seeded definition text includes the music cutoff, key/property return time, insurance requirement, and meeting versus wedding/concert/stage show/ball cancellation conditions. |
| Public navigation | Services and Documents export `publicNav`; registry combines these with Operations. The public header consumes registry links, including Public notices and Road issues map. |
| Multipart | Replaced S1's local byte parser with a thin error-envelope wrapper around `axum::extract::Multipart`; existing upload routes retain their call pattern. Multipart upload tests and HTTP smoke pass. |
| Shared mobile header | Made the wordmark compact at small widths, kept controls from shrinking, and moved the persona link's responsive visibility to a wrapper to avoid conflicting display classes. Public and signed-in staff headers were visually checked at 360 pixels. |

The requested Cargo features (`axum/multipart`, `printpdf/embedded_images`) and dev debug setting were already enabled. Deployment changes belong to the other agent. Worktree cleanup and finance integration were left to their owners.

## Consistency audit and fixes

- Every seeded non-finance module handler reaches its owner's `step_guard_handler`. The finance handler remains explicitly deferred. Regression coverage invokes the real Operations and Documents implementations.
- All four seeded decision types have S2 templates and are issuable: development approval, building approval, modification approval, and planning certificate. The building smoke actually issues the first two. Removed road/complaint response values from the decision-step validation allowlist: S2 issues those through letter handlers.
- All six seeded task kinds are accepted by `operations::api::create_step_task`. The audit creates each kind through that API; the smoke completes road inspection and repair tasks as Jake.
- Feature panel keys are unique, including against core case tabs. Tests mount every applicable registered panel through the actual staff and applicant case pages for all seven service modules, using `/staff/cases/:id` and `/my/cases/:id`. Leaflet itself is mocked in this rendering test. Browser checks also confirmed staff Decisions, Records and Tasks, and applicant Documents panels.
- The existing workflow close path already calls `records::api::on_case_closed`; no extra call was added. Road closure produces an accepted integration delivery in the smoke.
- The existing decision issuance path already calls `deadlines::api::on_trigger("decision_issued")` and records' decision hook. No duplicate trigger was added.
- Found and fixed the hall `instructions`/`setup_notes` mismatch and notice `building_approval`/`project_reference` mismatch, including the notice definition validator.
- Found and fixed premature required-action resolution on a text-only applicant reply and replacement uploads failing to resume the deadline clock.
- Verified a revoked organisation member gets 404 from S2's real document download route while the organisation owner retains access. Both Rust route tests and HTTP smoke cover this.
- Disposal tests verify unchanged immutable evidence/version IDs, shared bytes surviving the first disposal, bytes disappearing through GC after the final consumer is disposed, and foreign-key integrity.

## Validation

All required commands passed:

```text
cd server && cargo test
  78 passed; 0 failed; 3 existing documentation examples ignored
cd server && cargo clippy --all-targets -- -D warnings
  passed
cd web && npm run build && npm run lint && npm test
  passed; 27 tests in 3 files
scripts/smoke-integration.sh
  PASS integration smoke
git diff --check
  passed
```

Vite reports its existing advisory about a bundle exceeding 500 kB; the build succeeds. A path-scoped diff check confirms no changes under the forbidden finance/payment/deployment paths.

## Local HTTP smoke

`scripts/smoke-integration.sh` builds the frontend, runs `seed-demo`, starts `serve` on a free port with `DEMO_MODE=true`, `COOKIE_SECURE=false`, and `WEB_DIST`, and exercises requests using curl. Staff logins complete TOTP through `/api/demo/authenticator`. Its default database is a disposable `/tmp/nsh-int.*` directory; `SMOKE_DATA_DIR=/tmp/nsh-int` retains a run's database and server log. The smoke also passed with that explicit data directory. It cleans up its server on exit.

Planning intake cannot persist advancement yet: entering the following payment step calls the finance invoice stub, which returns an error. The smoke asserts atomic rollback and reports the finance skip rather than treating this expected stub response as an integrated payment workflow.

Final fresh-database output:

```text
ok   resident planning certificate submitted; intake advance attempted; atomic rollback at invoice stub
SKIP (finance pending): planning payment, certificate preparation and issuance
ok   building intake → replacement request → text reply keeps action → v2 clears action/resumes clock → two Priya approvals → completed
ok   canonical original_approval/project_reference answers link modification and both notices to the building project
ok   road report → Jake inspection done → Jake repair done → response letter → closed → integration delivered
ok   confidential complaint submitted/assigned → Olga 404 → generic outbound → completed → independent review copies exclusions
ok   revoked organisation member immediately receives 404 on the real document download; owner retains access
ok   historical legacy import preserves Norfolk dates; replay is idempotent; repeat upload detects exact duplicate; S5 disposal succeeds
ok   S5 user deactivation immediately invalidates the resident session
PASS integration smoke
```
