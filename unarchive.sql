-- Use this script to unread all entries.
BEGIN TRANSACTION;
INSERT INTO stash_1 (uri, title, time_add)
SELECT uri, title, time_add
FROM archive_1;

DELETE FROM archive_1;

COMMIT;
