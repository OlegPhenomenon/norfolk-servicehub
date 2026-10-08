-- Manager withdrawal of a published exhibition is recorded with its mandatory reason (who, when). Like a formal
-- termination, comments already received still need a consideration outcome before the exhibition step passes.
ALTER TABLE exhibitions ADD COLUMN withdrawn_at TEXT;
ALTER TABLE exhibitions ADD COLUMN withdrawn_by INTEGER REFERENCES users(id);
ALTER TABLE exhibitions ADD COLUMN withdrawal_reason TEXT;
