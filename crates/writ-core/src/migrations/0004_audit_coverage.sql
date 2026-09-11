-- What the gate needs to stop re-nagging a diff it already covered.
-- Spec section 9.2, **The gate does not re-nag a diff it already
-- covered**.
--
-- `stop_hook_active` bounds one stop-cycle and the next user message
-- clears it, so an unchanged diff that was audited and answered was
-- selected again on the very next turn, and again after that. The range
-- could not key that state. The diff itself can.
--
-- Both columns are nullable, which is what makes this cheap to retrofit
-- (P9) and is also the truth: audits written before this migration were
-- built over a diff nobody hashed, and no value would be honest there.
-- A NULL digest matches nothing, so those rows simply never cover
-- anything. Nothing is dropped and nothing is backfilled — contrast
-- schema 3, where the column had to be NOT NULL because a read path
-- depended on it.

-- A hash of the diff text the audit was built over. Not of the prompt:
-- the prompt carries the rendered rules, so a `max_chars` change or a
-- rule edit would alter it and re-nag a diff nobody touched.
ALTER TABLE audits ADD COLUMN diff_digest TEXT;

-- When findings were ingested against this row. `findings` cannot answer
-- that question: it is a count, and an agent that checked the diff and
-- honestly found nothing ingests zero of them. Reading `findings = 0` as
-- "never answered" would re-nag precisely the agent that did the work.
ALTER TABLE audits ADD COLUMN ingested_at TEXT;

-- The gate's only read of these columns, on every gated turn.
CREATE INDEX IF NOT EXISTS audits_coverage
    ON audits (diff_digest, repo)
 WHERE diff_digest IS NOT NULL AND ingested_at IS NOT NULL;
