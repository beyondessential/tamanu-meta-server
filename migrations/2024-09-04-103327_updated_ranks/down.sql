CREATE OR REPLACE VIEW latest_statuses AS (
	with
	successes as (
		select id, created_at, server_id, latency_ms, version
		from (
			select *,
			row_number() over(partition by server_id order by created_at desc) as rn
			from statuses
			where error is null
		) t
		where t.rn = 1
	),
	errors as (
		select id, created_at, server_id, latency_ms, error
		from (
			select *,
			row_number() over(partition by server_id order by created_at desc) as rn
			from statuses
			where error is not null
		) t
		where t.rn = 1
	)
	select
		servers.id as server_id,
		servers.created_at as server_created_at,
		servers.updated_at as server_updated_at,
		servers.name as server_name,
		servers.rank as server_rank,
		servers.host as server_host,

		COALESCE((errors IS NULL AND successes IS NOT NULL) OR (successes.created_at > errors.created_at), FALSE) as is_up,
		CASE
			WHEN (errors.latency_ms >= 10000) THEN NULL
			ELSE COALESCE(successes.latency_ms, errors.latency_ms)
		END as latest_latency,

		successes.id as latest_success_id,
		successes.created_at as latest_success_ts,
		(current_timestamp - successes.created_at) as latest_success_ago,
		successes.version as latest_success_version,

		errors.id as latest_error_id,
		errors.created_at as latest_error_ts,
		(current_timestamp - errors.created_at) as latest_error_ago,
		errors.error as latest_error_message
	from servers
	left join successes on servers.id = successes.server_id
	left join errors on servers.id = errors.server_id
	order by (
     CASE
       WHEN (servers.rank = 'live') THEN 0
       WHEN (servers.rank = 'demo') THEN 1
       WHEN (servers.rank = 'dev') THEN 2
       ELSE 3
     END
    ), servers.name
);
