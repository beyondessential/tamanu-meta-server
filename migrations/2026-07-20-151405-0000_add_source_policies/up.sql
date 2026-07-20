-- Per-source operator policy. Currently the reachability mode (on/quiet/
-- off); the ingest mode is added by a later migration.
create table source_policies (
	source text primary key,
	reachability text not null default 'on',
	created_at timestamptz not null default now(),
	updated_at timestamptz not null default now()
);

select diesel_manage_updated_at('source_policies');

-- The legacy Tamanu heartbeat is effectively synthetic (canopy fabricates
-- the `tamanu` source from legacy pushes) and now reports on only part of
-- the fleet. Quiet it: a stale tamanu never warns, but a legacy-only
-- server going silent still reads unreachable.
insert into source_policies (source, reachability) values ('tamanu', 'quiet');
