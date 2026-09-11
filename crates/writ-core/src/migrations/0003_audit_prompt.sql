-- What the audit actually sent, kept so a gate can hand the agent a short
-- pointer and let the agent fetch the document by id. Spec section 9.2,
-- **The gate points, it does not paste**.
--
-- `NOT NULL`, so there is no such thing as an audit whose prompt is
-- missing and no read path that has to decide what an absent one means.
-- SQLite cannot add a NOT NULL column to a populated table without a
-- default, and the only candidate is `''` — a value `--fetch` would hand
-- an agent as a document. P7 forbids that, so the table is rebuilt and
-- the rows that predate the column are dropped rather than backfilled
-- with a value that is not true. There is nothing to recover them from:
-- the prompt was never stored and the diffs are gone.
--
-- `findings` goes with them. It is `ON DELETE CASCADE` from `audits`, and
-- a finding without its audit is a report about nothing.

DELETE FROM findings;
DROP TABLE audits;

CREATE TABLE audits (
  id          TEXT PRIMARY KEY,             -- UUIDv7
  started_at  TEXT NOT NULL DEFAULT (datetime('now')),
  repo        TEXT,
  diff_range  TEXT,
  considered  INTEGER NOT NULL DEFAULT 0,
  sent        INTEGER NOT NULL DEFAULT 0,
  findings    INTEGER NOT NULL DEFAULT 0,
  prompt      TEXT NOT NULL                 -- the document the audit sent
);
