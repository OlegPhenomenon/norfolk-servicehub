-- Contract fixes after architecture review.

-- Overlap protection must also hold when an existing price interval is edited (e.g. closing it with effective_to).
CREATE TRIGGER price_versions_no_overlap_update
BEFORE UPDATE OF price_item_id, effective_from, effective_to ON price_versions
BEGIN
    SELECT RAISE(ABORT, 'price_overlap')
    WHERE EXISTS (
        SELECT 1 FROM price_versions p
        WHERE p.id <> NEW.id
          AND p.price_item_id = NEW.price_item_id
          AND (p.effective_to IS NULL OR p.effective_to > NEW.effective_from)
          AND (NEW.effective_to IS NULL OR NEW.effective_to > p.effective_from)
    );
END;

-- Exact document versions that were attached when the request was submitted (owner: services).
CREATE TABLE submission_documents (
    submission_id           INTEGER NOT NULL REFERENCES submissions(id),
    document_version_id     INTEGER NOT NULL REFERENCES document_versions(id),
    requirement_key         TEXT,
    PRIMARY KEY (submission_id, document_version_id)
);

-- Each time a case enters a workflow step (owner: services). Re-entering a step (reopen) creates a new run.
CREATE TABLE workflow_step_runs (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    step_key        TEXT NOT NULL,
    entered_at      TEXT NOT NULL,
    left_at         TEXT,
    left_reason     TEXT CHECK (left_reason IN ('advanced', 'skipped', 'refused', 'cancelled', 'withdrawn', 'reopened'))
);
CREATE INDEX workflow_step_runs_case ON workflow_step_runs(case_id, step_key);

-- Tasks belong to the step run that created them (owner: operations).
ALTER TABLE tasks ADD COLUMN step_run_id INTEGER REFERENCES workflow_step_runs(id);

-- The issued artefact is a specific immutable version, not a mutable document (owner: documents).
ALTER TABLE decisions ADD COLUMN output_document_version_id INTEGER REFERENCES document_versions(id);

-- Hourly lines: amount is computed from exact minutes, never from rounded thousandths of an hour (owner: finance).
ALTER TABLE invoice_lines ADD COLUMN quantity_minutes INTEGER;
