-- Where each session's current turn found each repository. Spec section
-- 9.2, **The Stop gate audits only what the turn changed**.
--
-- Stop used to audit whatever repository the session's directory was in,
-- so a session that only ran `cd` into a worktree another session had
-- left dirty was sent to review that work. A row here is the tree a
-- repository held when the turn first reached it, and Stop passes when
-- the tree is still the same.
--
-- `turns` says the host records turns for this session at all. Without
-- its row the gate behaves as it did before, so a host with no turn hook
-- loses nothing.
--
-- This is not evidence. A new prompt replaces the session's rows, and
-- old sessions are pruned.
CREATE TABLE turns (
  session_id TEXT PRIMARY KEY,
  started_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- `tree` is NULL when git could not snapshot the repository. Stop then
-- cannot tell what changed, so it audits.
CREATE TABLE turn_trees (
  session_id TEXT NOT NULL REFERENCES turns (session_id) ON DELETE CASCADE,
  root       TEXT NOT NULL,
  tree       TEXT,
  PRIMARY KEY (session_id, root)
);
