-- S2-owned ancillary state; immutable source versions remain in document_versions.
CREATE TABLE issued_letters (
 id INTEGER PRIMARY KEY, case_id INTEGER NOT NULL REFERENCES cases(id),
 letter_type TEXT NOT NULL CHECK(letter_type IN ('complaint_response','road_response')),
 document_id INTEGER NOT NULL REFERENCES documents(id),
 document_version_id INTEGER NOT NULL REFERENCES document_versions(id),
 issued_by INTEGER REFERENCES users(id), issued_at TEXT NOT NULL
);
CREATE INDEX issued_letters_case ON issued_letters(case_id,letter_type);
ALTER TABLE document_comments ADD COLUMN request_new_version INTEGER NOT NULL DEFAULT 0 CHECK(request_new_version IN (0,1));
ALTER TABLE document_comments ADD COLUMN message_id INTEGER REFERENCES case_messages(id);
CREATE TABLE building_original_approvals (
 case_id INTEGER PRIMARY KEY REFERENCES cases(id), decision_id INTEGER NOT NULL REFERENCES decisions(id)
);
CREATE TABLE documents_seed_files (
 name TEXT PRIMARY KEY, blob_id INTEGER NOT NULL REFERENCES blobs(id)
);
CREATE TRIGGER document_version_immutable BEFORE UPDATE ON document_versions
BEGIN SELECT RAISE(ABORT,'document versions are immutable'); END;
CREATE TRIGGER issued_decision_immutable BEFORE UPDATE ON decisions WHEN OLD.status='issued'
BEGIN SELECT RAISE(ABORT,'issued decisions are immutable'); END;
CREATE TRIGGER issued_evidence_immutable_delete BEFORE DELETE ON decision_evidence
WHEN EXISTS(SELECT 1 FROM decisions WHERE id=OLD.decision_id AND status='issued')
BEGIN SELECT RAISE(ABORT,'issued evidence is immutable'); END;
CREATE TRIGGER issued_evidence_immutable_insert BEFORE INSERT ON decision_evidence
WHEN EXISTS(SELECT 1 FROM decisions WHERE id=NEW.decision_id AND status='issued')
BEGIN SELECT RAISE(ABORT,'issued evidence is immutable'); END;

CREATE TRIGGER issued_evidence_immutable_update BEFORE UPDATE ON decision_evidence
WHEN EXISTS(SELECT 1 FROM decisions WHERE id IN (OLD.decision_id,NEW.decision_id) AND status='issued')
BEGIN SELECT RAISE(ABORT,'issued evidence is immutable'); END;
