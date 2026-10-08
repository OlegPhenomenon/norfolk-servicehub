-- Catalogue provenance for versioned upgrades of the seeded service catalogue (services::seed::upgrade).
-- `seed_hash` is the SHA-256 of the canonical seeded definition JSON. A version whose stored definition still
-- hashes to its `seed_hash` was produced by the catalogue seed; NULL means staff created it in the builder.
ALTER TABLE service_versions ADD COLUMN seed_hash TEXT;

-- Versions written by the seed before this migration: no creator, never published by a person, and the
-- seed's source note. Their content is not re-hashed here; the upgrade treats 'legacy' as seed provenance
-- for published (immutable) versions only.
UPDATE service_versions SET seed_hash = 'legacy'
WHERE created_by IS NULL AND published_by IS NULL
  AND source_note LIKE 'Fictional demonstration configuration based on NIRC form:%';
