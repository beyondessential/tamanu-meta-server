UPDATE server_group_silenced_refs SET source = 'status' WHERE source = 'alertd';
UPDATE server_silenced_refs SET source = 'status' WHERE source = 'alertd';
UPDATE issues SET source = 'status' WHERE source = 'alertd';

ALTER TABLE statuses DROP COLUMN source;
