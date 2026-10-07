-- Enforce immutable published configuration and submission snapshots, including accidental SQL edits.
CREATE TRIGGER service_version_definition_immutable
BEFORE UPDATE OF definition_json, source_blob_id, source_note, service_id, version ON service_versions
WHEN OLD.status <> 'draft'
BEGIN
  SELECT RAISE(ABORT, 'published_service_immutable');
END;
CREATE TRIGGER submission_snapshot_immutable
BEFORE UPDATE ON submissions
BEGIN
  SELECT RAISE(ABORT, 'submission_immutable');
END;
CREATE UNIQUE INDEX deadline_one_policy_per_case ON deadlines(case_id, kind);
CREATE UNIQUE INDEX deadline_one_open_pause ON deadline_pauses(deadline_id) WHERE ended_at IS NULL;
CREATE UNIQUE INDEX workflow_one_open_run ON workflow_step_runs(case_id) WHERE left_at IS NULL;
