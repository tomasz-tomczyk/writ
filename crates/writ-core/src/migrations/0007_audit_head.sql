-- The commit an audit was emitted at. Spec section 9.2, **Changed since
-- your last answer**.
--
-- A gate that fires again on one range is usually firing on the agent's
-- own fix, and the prompt asked for the whole branch again. With the
-- commit of the answered audit on its row, the next prompt can name the
-- paths that changed since and the command that prints what changed.
--
-- Nullable, which is P9's cheap case and also the truth: a repository with
-- no commit has no HEAD, and the audits written before this migration
-- recorded none. A NULL head is never an answer to compare against, so
-- those rows simply never produce the section.
ALTER TABLE audits ADD COLUMN head TEXT;

-- The prompt's only read of it: the newest answered audit of one range in
-- one repository, on every emitted audit.
CREATE INDEX IF NOT EXISTS audits_last_answer
    ON audits (repo, diff_range, id)
 WHERE ingested_at IS NOT NULL AND head IS NOT NULL;
