-- Which half of the diff a learning cares about. Default `both` keeps the
-- historical matcher behaviour (added or removed). Spec section 6 / 8.1.

ALTER TABLE learnings ADD COLUMN sides TEXT NOT NULL DEFAULT 'both'
  CHECK (sides IN ('added', 'removed', 'both'));
