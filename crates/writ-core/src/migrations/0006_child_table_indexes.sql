-- Two indexes the gate path reads on every turn, and one it no longer
-- reads at all.
--
-- Neither table carried an index on the column every hot query filters
-- by, so both planned as a full scan. The cost does not grow with the
-- diff, which is why a benchmark over one repository never showed it: it
-- grows with the collection and with the age of the ledger.

-- `Store::exemplars_of` runs once per ranked candidate inside the budget
-- loop, so a gated turn planned up to `max_rules` full scans of
-- `exemplars`. The primary key starts with `id`, which answers no
-- question the audit asks.
CREATE INDEX IF NOT EXISTS exemplars_by_learning
    ON exemplars (learning_id, id);

-- `Store::outcomes_of_many` grouped over a full scan of `findings` plus a
-- temporary b-tree. It is read once per audit, but it reads every finding
-- ever written, so it degrades with the ledger's age rather than with the
-- work in front of it.
CREATE INDEX IF NOT EXISTS findings_by_learning
    ON findings (learning_id, outcome);

-- Schema 5 moved the gate's coverage read to `audit_coverage`, keyed per
-- learning on its own slice. Nothing selects `audits.diff_digest` any
-- more, so this index was pure write cost on every `start_audit`. The
-- column stays: it is evidence of what an audit looked at, and P4 does
-- not destroy evidence.
DROP INDEX IF EXISTS audits_coverage;
