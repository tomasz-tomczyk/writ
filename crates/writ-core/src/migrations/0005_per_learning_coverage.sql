-- Coverage keyed per learning, over the slice of the diff that made that
-- learning match. Spec section 9.2, **The digest has to match the
-- granularity of selection**.
--
-- Schema 4 keyed coverage on a hash of the *whole* diff, while selection
-- matches per learning, per path. When every active learning is `global`
-- those are the same thing. They come apart the moment a rule carries a
-- `glob:` scope and the gate carries a `merge-base` range, which is the
-- combination *What the gate audits* mandates.
--
-- Measured in the author's own ledger: eleven selections of one learning
-- scoped `glob:.github/workflows/**` across 75 minutes, the first
-- reporting three violations and each of the ten after it ingesting
-- clean. Eleven distinct digests, so coverage never matched once. Every
-- later turn edited a source file the rule does not scope, and the
-- whole-diff digest moved with it while the rule's own concern had not.
--
-- `audits.diff_digest` is left in place. It is evidence of what an audit
-- looked at (P4), and dropping a column to remove a redundancy is the
-- trade schema 3 already made at the cost of every row that predated it.
CREATE TABLE IF NOT EXISTS audit_coverage (
  audit_id     TEXT NOT NULL REFERENCES audits(id)    ON DELETE CASCADE,
  learning_id  TEXT NOT NULL REFERENCES learnings(id) ON DELETE CASCADE,
  -- Denormalized from `audits` so the gate's lookup is one index probe
  -- per selected learning instead of a join to find the repository.
  repo         TEXT,
  -- A hash of the learning's slice: the hunks of the paths its `glob:`
  -- and `language:` scopes selected, concatenated in sorted-path order
  -- so the same change hashes the same however git laid it out. A
  -- `global` scope's slice is the whole diff, which is deliberate — a
  -- global rule does care about every byte.
  slice_digest TEXT NOT NULL,
  PRIMARY KEY (audit_id, learning_id)
);

-- The gate's only read, once per selected learning on every gated turn.
-- `ingested_at` lives on the parent `audits` row, so this covers the
-- lookup and the join key both.
CREATE INDEX IF NOT EXISTS audit_coverage_lookup
    ON audit_coverage (repo, learning_id, slice_digest, audit_id);
