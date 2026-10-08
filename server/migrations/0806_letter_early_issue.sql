-- Letters issued before their step under the old rules (left unlinked by 0805) belong to the first run of a step
-- waiting for that letter type entered after them, if that run exists already and has no letter of that type.
-- Runs entered later adopt them on entry (documents::api::adopt_unlinked_letter). Owner: documents.
UPDATE issued_letters SET step_run_id = (
    SELECT r.id FROM workflow_step_runs r
    WHERE r.case_id = issued_letters.case_id
      AND r.entered_at >= issued_letters.issued_at
      AND r.step_key IN (
          SELECT json_extract(s.value, '$.key')
          FROM cases c,
               json_each(COALESCE((SELECT definition_snapshot_json FROM submissions WHERE case_id = c.id),
                                  (SELECT definition_json FROM service_versions WHERE id = c.service_version_id)),
                         '$.workflow.steps') s
          WHERE c.id = issued_letters.case_id
            AND json_extract(s.value, '$.handler') = 'documents.letter_issued:' || issued_letters.letter_type)
      AND NOT EXISTS (SELECT 1 FROM issued_letters o
                      WHERE o.step_run_id = r.id AND o.letter_type = issued_letters.letter_type)
    ORDER BY r.id LIMIT 1)
WHERE step_run_id IS NULL;
