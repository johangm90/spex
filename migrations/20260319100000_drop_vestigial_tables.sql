-- Drop vestigial tables that were created in the initial migration
-- but never used by any code path.
DROP TABLE IF EXISTS constitution;
DROP TABLE IF EXISTS meta;
