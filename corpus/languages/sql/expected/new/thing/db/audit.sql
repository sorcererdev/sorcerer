-- Audit.
CREATE TABLE audit_log (
  id integer PRIMARY KEY,
  note text
);

SELECT 'hole setup: the statement that fills audit_log to begin with';

SELECT id, note
FROM audit_log
LIMIT NULL /* hole limit: the most rows to read */;
