-- Audit N-04: the Builder offers documents.letter_issued:service_response to every module, so the generic
-- service response letter must be storable. SQLite cannot alter a CHECK constraint; rebuild the table.
CREATE TABLE issued_letters_new (
 id INTEGER PRIMARY KEY, case_id INTEGER NOT NULL REFERENCES cases(id),
 letter_type TEXT NOT NULL CHECK(letter_type IN ('complaint_response','road_response','service_response')),
 document_id INTEGER NOT NULL REFERENCES documents(id),
 document_version_id INTEGER NOT NULL REFERENCES document_versions(id),
 issued_by INTEGER REFERENCES users(id), issued_at TEXT NOT NULL
);
INSERT INTO issued_letters_new(id,case_id,letter_type,document_id,document_version_id,issued_by,issued_at)
 SELECT id,case_id,letter_type,document_id,document_version_id,issued_by,issued_at FROM issued_letters;
DROP TABLE issued_letters;
ALTER TABLE issued_letters_new RENAME TO issued_letters;
CREATE INDEX issued_letters_case ON issued_letters(case_id,letter_type);
