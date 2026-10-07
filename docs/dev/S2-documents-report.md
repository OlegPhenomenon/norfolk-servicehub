# S2 implementation handoff

Uncommitted implementation in the S2 worktree. The interrupted session's work was retained and completed. All builds and tests ran in place; no project copies were made.

## Implemented endpoints

- `POST /api/cases/{id}/documents`, `POST /api/documents/{id}/versions`, `GET /api/cases/{id}/documents`, `GET /api/document-versions/{vid}/download`, and version comments. Multipart fields include document requirements, category, title, staff visibility, notes, and comment resolutions. Documents retain all versions. Downloads recheck case access and document visibility, deny TaskOnly access, and send attachment/private/no-store headers. Finance-only users retain read access and cannot mutate documents. Closed cases reject uploads/comments. Replacement requests call the existing S1 message API with the exact version and `requires_response=true`; resolved comments retain the replacing version ID and notify their staff author.
- Decision template listing and case decision listing/preparation/editing/submission/return/issue. Development, building, modification and planning certificates have separate rows. Preparation defaults to each applicant document's latest version; evidence IDs are pinned. Issued decisions/evidence and document versions cannot be overwritten. Issuance checks an explicit active, service-scoped decision authority, creates an applicant PDF including exact evidence titles/version numbers, notifies the applicant, and calls the deadline, records and workflow APIs. A sysadmin role does not confer authority.
- `POST /api/cases/{id}/letters` and the contracted `attach_generated`, `issued_decisions`, `issue_letter`, and `letter_issued` APIs. Road/complaint response letters have their own issuance records and guard checks.
- `GET /api/my/issued-approvals`, `GET /api/building-projects/{id}`, building submit hooks, and the `decision_ref` validator. Initial applications create a project; modifications validate applicant access to an issued approval, link the case/project, and retain the superseded decision reference. Commencement/completion notices link to the selected project. Project views include accessible cases, link kinds, decisions and exact evidence versions.
- Staff exhibition listing/create/edit/detail, exact source item selection/editing, staff-only page previews, publication by a second staff member, submission listing and marking considered. Public exhibition listing/detail/published-file download and validated, rate-limited submissions. Reads and handler guards close expired windows. PDFs are rendered by Poppler at 110 dpi with a 30-second command timeout and 30-page cap; rectangles are burned into RGB pixels; new PDFs contain images only and have metadata removed. Image copies are re-encoded without source metadata or alpha. Public routes never select a source blob.

## Implemented pages

- Documents panel for applicant/staff: categories, full version history, uploader/date/note, downloads, replacement upload, resolved-comment selection, internal comments, and staff replacement requests.
- Decisions panel: versioned templates, reasons/conditions, exact evidence checkboxes, draft editing, submit/return/issue actions, explicit authority explanation, applicant downloads, and project links.
- Issued-approval field widget registered as `decision_ref`.
- `/my/projects/:id` and `/staff/projects/:id`: project requests, modification/follow-up links, decision history, issue dates and evidence version chips.
- `/staff/exhibitions` and `/staff/exhibitions/:id`: notice/window editor, chosen document versions, source page previews, pointer rectangle drawing, labelled numeric keyboard controls, independent publication approval, and submission review.
- `/notices` and `/notices/:id`: public notice windows, published copies, and submission form. Relative registry routes and staff Exhibitions navigation follow the platform conventions.

## Migration and seeds

`0201_documents.sql` adds issued letters, replacement-request linkage, original-approval linkage, sample-file references and immutability triggers. No base migrations were edited.

Seeds are idempotent, include version 1 templates for all six canonical types, and generate clearly fictional schematic site/elevation PDFs. Small generated copies are under `server/seed-data/docs/`. The elevation includes the requested confidential phone marker for the redaction demonstration. To regenerate sample copies from the helper, run the seed-idempotence test with `SERVICEHUB_EXPORT_DOCUMENT_SAMPLES=1` (and the dependency feature flags below).

## Verification

- Default `cargo test`: failed to compile because shared Cargo.toml does not enable axum multipart or printpdf embedded_images.
- Default `cargo clippy -- -D warnings`: same two missing feature errors.
- `cargo test --features axum/multipart,printpdf/embedded_images`: all 46 unit tests passed, including 14 S2 tests; three pre-existing documentation examples are ignored.
- `cargo clippy --features axum/multipart,printpdf/embedded_images -- -D warnings`: passed.
- `npm run build`: passed.
- `npm run lint`: passed with zero warnings.
- `git diff --check`: passed.
- Chrome smoke check: public-notices page and its empty state rendered successfully. Temporary server, database and browser tab were cleaned up. A full 360 px UI walkthrough was not performed.

Tests cover access projection/revocation, TaskOnly and finance restrictions, replacement resolution/rollback, immutable evidence, distinct approval/refusal rows, explicit authority denial, project/modification linkage, certificate reissue preservation and PDF contents, public source exclusion, comment windows, independent publication approval, opaque image redaction, burned PDF pixels, and completely empty public PDF text extraction. Poppler was installed, so the redaction/content tests ran rather than skipping.

## Deviations and integration limits

- The permitted hook signature has no actor argument: `validate_field` validates canonical decision-reference structure and issued approval state; the building submit hook performs applicant-access validation for `original_approval` and project links.
- Project access is checked per case before returning each case/decision/link, preserving explicit denials within an otherwise visible project.
- Exhibition windows close on reads and guard evaluation, as requested; no additional background-job dispatcher was introduced.
- S1 message/deadline/workflow and S5 records APIs are still stubs in this isolated checkout. The prescribed calls remain atomic and compile, but successful end-to-end replacement requests and final issue/letter endpoints require the merged implementations. S2 tests exercise its own issuance helpers and seed their own rows without depending on those slices.

## Requests for other slices

1. **Platform/orchestrator: required build fix.** Enable existing dependency features in shared `server/Cargo.toml`:
   ```toml
   axum = { version = "0.8", features = ["multipart"] }
   printpdf = { version = "0.7", features = ["embedded_images"] }
   ```
   Cargo.toml/Cargo.lock were left untouched under the ownership rule. An ownership exception was requested during the session, but no approval arrived. The feature-qualified commands above prove the implementation builds and passes. After enabling the features, rerun the two default Rust commands.
2. **S1:** implement the existing message/deadline/workflow signatures. For required-action/clock resolution, S2 stores `document_comments.message_id`, `resolved_at`, and `resolved_by_version_id`; derive replacement-request completion from these rows so a replaced drawing does not remain an outstanding applicant action. Do not resume unrelated outstanding requests.
3. **S1 catalogue:** modification field `original_approval` uses canonical `decision_ref` (`{"decision_id":123}`); notice field `project_reference` accepts the project reference string or ID. Certificate section information accepts `sections` or `certificate_sections` from submitted answers. The hooks use the canonical service slugs.
4. **S3/S5:** response-letter callers can use the existing `documents::api::issue_letter` signature or the case-letter endpoint. It records issuance, notifies the applicant and attempts workflow advancement. The existing `records::api::on_decision_issued` callback is used on approval/certificate issuance.

No new cross-module API signatures are requested. No commit was created.
