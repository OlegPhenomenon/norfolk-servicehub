# S1 handoff

Implemented in the S1 worktree without committing. No platform or other-slice files were edited.

## Backend

- Public service list/detail, FTS prefix search and synonyms, category filtering, published definitions and effective price projections.
- Definition and answer validation, conditional fields, canonical module checkpoints, draft version editing, validation, preview answers, immutable publishing, PDF source attachment/upload, price-item picker and version history.
- Bulk JSON import validation reports and idempotent apply creating valid draft versions only. `server/seed-data/service-import-sample.json` contains three researched forms, with one deliberately invalid item.
- Optional mock AI PDF suggestion flow, bounded extraction, deterministic suggestions, source provenance, empty pricing/deadlines and warnings; disabled endpoint returns 404.
- Resident and assisted drafts, autosave, document requirements, version-pinned submission snapshots, SHA-256, serialized idempotent submission, numbering, hooks, deadlines, events, audit and applicant notifications.
- Case projection and server-computed actions; advance, optional skip, information requests, refusal, withdrawal, cancellation, reopening and duplicate closure. Generic payment/task entry and guard work goes through the platform hooks.
- Assignments, atomic owner replacement, ending assignments, escalation, staff user picker, applicant-visible messages, internal notes, representative authorization/revocation and scoped paginated case search.
- Frozen deadline policies, Norfolk 17:00 due dates, business/calendar arithmetic, separate completeness clock, cumulative pause caps, resume events, auto-resume and once-only breach notifications.
- Idempotent seeds for all 12 canonical services; 11 published, dog registration draft. Real form names, fields, document lists and source URLs; illustrative workflow time targets clearly marked.
- Migration `0101_service_invariants.sql` protects submission/version immutability and open deadline/step invariants.

## Frontend

- `/services`, `/services/:slug`: catalogue/search/categories and service explanation, documents, live prices and steps.
- `/my`, `/my/drafts/:id`, `/my/cases/:id`: paginated dashboard, autosave/review/submission, document slots, required reply, messages, timeline, representatives and registered applicant panels.
- `/staff`, `/staff/cases`, `/staff/cases/:id`, `/staff/intake`: queues/search, case workspace/actions, frozen labelled answers, messages, internal notes, assignments, deadlines, registered panels and one-screen assisted intake.
- `/admin/services`, `/admin/services/new`, `/admin/services/:id`, `/admin/service-imports`: structured builder, draft preview, validation, publishing/history, original-form upload, mock AI suggestion and import/report/apply.
- Feature navigation and relative route exports follow the existing registry. Core UI components and custom field/panel registries are reused.

## Verification

- `cargo test`: 44 passed, zero failed; three existing documentation examples ignored.
- `cargo clippy -- -D warnings`: passed.
- `npm run build`: passed.
- `npm run lint`: passed with zero warnings.
- `git diff --check`: passed.
- Browser: published catalogue, synonym search (`party`), hall service detail; catalogue/detail tested at 360 px with no horizontal overflow. Temporary viewport restored and review server stopped.
- S1 invariants cover concurrent idempotency, immutable snapshots after v2 publication, required document/version freezing, conditional validation, role/revision actions, internal-note isolation, holiday/weekend pauses, cumulative calendar caps, delayed sweeps, imports, published edit rejection, AI-disabled behavior, seed idempotence, synonyms and binary multipart boundaries.

## Deviations and merge follow-up

- `validate_definition` returns `AppResult<Vec<ValidationIssue>>` so database failures are preserved instead of silently becoming validation success.
- Draft deletion removes the draft and tombstones its case as withdrawn; retained events and uploaded-document references are preserved, and unsubmitted tombstones are excluded from resident lists.
- Assisted intake additionally supports `draft_only` and `case_id`, allowing upload before final submission on the same screen. Original PDF source upload additionally has a `source-file` endpoint.
- Axum's platform dependency lacks the multipart feature. S1 uses a bounded local upload extractor rather than changing the unowned Cargo manifest. S0 should enable Axum multipart and replace this adapter when integrating.
- S2–S4 and records APIs are still stubs in this isolated tree. Payment settlement, decision refusal, task/module guards, close-to-records and full cross-module browser journeys require merged integration verification. The requested direct-row settled-payment test cannot pass against the current finance stub; no parallel implementation was introduced.
- S0: the shared mobile header overlaps its logo and persona link at 360 px. It is outside S1 ownership and was left unchanged.
- No new cross-module API signature is requested. Existing hooks/APIs and frontend registries are used.
