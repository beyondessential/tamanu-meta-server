-- Second dimension of source policy: whether the device API ingests a
-- source's reports (allow), silently drops them (ignore), or rejects the
-- push (deny).
alter table source_policies
	add column ingest text not null default 'allow';
