CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS counters (
  day     TEXT NOT NULL,
  metric  TEXT NOT NULL,
  label   TEXT NOT NULL DEFAULT '',
  count   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, metric, label)
);

CREATE TABLE IF NOT EXISTS buckets (
  day     TEXT NOT NULL,
  metric  TEXT NOT NULL,
  bucket  TEXT NOT NULL,
  count   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, metric, bucket)
);
