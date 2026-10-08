-- A response letter satisfies only the workflow step run it was issued at (owner: documents).
-- Back-fill: link each existing letter to the latest run, entered no later than the letter, of a step of the
-- case's frozen workflow that waits for that letter type. Letters with no such run stay NULL; they never
-- satisfy an open step and only document legacy (already closed) cases.
ALTER TABLE issued_letters ADD COLUMN step_run_id INTEGER REFERENCES workflow_step_runs(id);
UPDATE issued_letters SET step_run_id = (
    SELECT r.id FROM workflow_step_runs r
    WHERE r.case_id = issued_letters.case_id
      AND r.entered_at <= issued_letters.issued_at
      AND r.step_key IN (
          SELECT json_extract(s.value, '$.key')
          FROM cases c,
               json_each(COALESCE((SELECT definition_snapshot_json FROM submissions WHERE case_id = c.id),
                                  (SELECT definition_json FROM service_versions WHERE id = c.service_version_id)),
                         '$.workflow.steps') s
          WHERE c.id = issued_letters.case_id
            AND json_extract(s.value, '$.handler') = 'documents.letter_issued:' || issued_letters.letter_type)
    ORDER BY r.id DESC LIMIT 1);
CREATE INDEX issued_letters_step_run ON issued_letters(step_run_id);
