-- Memory linking: store related memory keys as a JSON array.
ALTER TABLE memory ADD COLUMN related_to TEXT NOT NULL DEFAULT '[]';
