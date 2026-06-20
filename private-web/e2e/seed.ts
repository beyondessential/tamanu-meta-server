// Seeding helpers for Playwright e2e specs. Connect once per worker
// to the per-worker Postgres database; expose typed insert helpers
// for the rows the UI tests need to see. Each helper returns the
// inserted row's id (and a few readable fields) so the test can
// assert on the value it just produced.
//
// We talk to the database directly rather than going through the
// private-server API because (a) most state we want isn't reachable
// over admin endpoints (statuses, issues, devices in arbitrary
// roles) and (b) the API enforces validation and side-effects we
// often don't want in setup (e.g. submitting a status fires events
// and opens incidents).

import { randomBytes, randomUUID } from "node:crypto";
import { Client } from "pg";

export interface Sql {
	query<R extends Record<string, unknown> = Record<string, unknown>>(
		text: string,
		params?: unknown[],
	): Promise<R[]>;
	end(): Promise<void>;
}

export async function connect(databaseUrl: string): Promise<Sql> {
	const client = new Client({ connectionString: databaseUrl });
	await client.connect();
	return {
		async query<R extends Record<string, unknown> = Record<string, unknown>>(
			text: string,
			params: unknown[] = [],
		): Promise<R[]> {
			const result = await client.query(text, params);
			return result.rows as R[];
		},
		end: () => client.end(),
	};
}

function randomLabel(prefix: string): string {
	return `${prefix}-${randomBytes(4).toString("hex")}`;
}

/** Wipe every table this suite seeds into. Use from a `beforeEach`
 * when a test needs to make assertions that depend on the absence
 * of data (the "empty state" UI banners, mostly). Fast — single
 * statement with CASCADE. */
export async function resetSeededTables(sql: Sql): Promise<void> {
	await sql.query(
		"TRUNCATE statuses, issues, device_keys, servers, server_groups, devices, versions, tailscale_users, server_group_backup_config, server_group_backup_schedule, server_backup_capabilities, backup_requests, backup_runs, backup_repo_stats, backup_maintenance_runs RESTART IDENTITY CASCADE",
	);
}

export interface SeededServerGroup {
	id: string;
	name: string;
}

export async function seedServerGroup(
	sql: Sql,
	opts: {
		name?: string;
		notes?: string;
		tags?: Record<string, string>;
		/** Slack open cooldown in seconds. Omit to keep the migration default. */
		slackOpenDelaySeconds?: number;
	} = {},
): Promise<SeededServerGroup> {
	const id = randomUUID();
	const name = opts.name ?? randomLabel("group");
	if (opts.slackOpenDelaySeconds !== undefined) {
		await sql.query(
			`INSERT INTO server_groups (id, name, notes, tags, slack_open_delay)
			 VALUES ($1, $2, $3, $4::jsonb, make_interval(secs => $5))`,
			[
				id,
				name,
				opts.notes ?? "",
				JSON.stringify(opts.tags ?? {}),
				opts.slackOpenDelaySeconds,
			],
		);
	} else {
		await sql.query(
			`INSERT INTO server_groups (id, name, notes, tags)
			 VALUES ($1, $2, $3, $4::jsonb)`,
			[id, name, opts.notes ?? "", JSON.stringify(opts.tags ?? {})],
		);
	}
	return { id, name };
}

export type ServerRank = "production" | "clone" | "demo" | "test" | "dev";

export interface SeededServer {
	id: string;
	name: string;
	host: string;
	kind: "central" | "facility";
	rank: ServerRank | null;
}

export async function seedServer(
	sql: Sql,
	opts: {
		name?: string;
		host?: string;
		kind?: "central" | "facility";
		rank?: ServerRank | null;
		groupId?: string | null;
		deviceId?: string;
		notes?: string;
		tags?: Record<string, string>;
		/** Whether canopy actively monitors this server. Off by default so
		 * e2e seeds don't accidentally trip the reachability sweep. */
		isMonitored?: boolean;
		/** Threshold in seconds; defaults to 600 (10 min). Must be > 0. */
		alertWhenDownFor?: number;
	} = {},
): Promise<SeededServer> {
	const id = randomUUID();
	const name = opts.name ?? randomLabel("srv");
	const host = opts.host ?? `https://${randomLabel("host")}.e2e.invalid`;
	const kind = opts.kind ?? "central";
	const rank = opts.rank ?? "production";
	const isMonitored = opts.isMonitored ?? false;
	const alertWhenDownFor = opts.alertWhenDownFor ?? 600;
	await sql.query(
		`INSERT INTO servers (id, name, host, kind, rank, group_id, device_id, is_monitored, alert_when_down_for, notes, tags)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb)`,
		[
			id,
			name,
			host,
			kind,
			rank,
			opts.groupId ?? null,
			opts.deviceId ?? null,
			isMonitored,
			alertWhenDownFor,
			opts.notes ?? "",
			JSON.stringify(opts.tags ?? {}),
		],
	);
	return { id, name, host, kind, rank };
}

export interface SeededDevice {
	id: string;
	role: string;
}

export async function seedDevice(
	sql: Sql,
	opts: { role?: string } = {},
): Promise<SeededDevice> {
	const id = randomUUID();
	const role = opts.role ?? "server";
	await sql.query(`INSERT INTO devices (id, role) VALUES ($1, $2)`, [id, role]);
	return { id, role };
}

/** Add a device_key row so the device shows up as trusted (active
 * key) or untrusted (inactive key). */
export async function seedDeviceKey(
	sql: Sql,
	opts: {
		deviceId: string;
		name?: string;
		isActive?: boolean;
		keyData?: Buffer;
	},
): Promise<{ id: string }> {
	const id = randomUUID();
	await sql.query(
		`INSERT INTO device_keys (id, device_id, name, is_active, key_data)
		 VALUES ($1, $2, $3, $4, $5)`,
		[
			id,
			opts.deviceId,
			opts.name ?? randomLabel("key"),
			opts.isActive ?? true,
			opts.keyData ?? randomBytes(32),
		],
	);
	return { id };
}

export interface SeededStatus {
	id: string;
	createdAt: string;
}

export async function seedStatus(
	sql: Sql,
	opts: {
		serverId: string;
		deviceId?: string | null;
		version?: string | null;
		healthy?: boolean;
		health?: unknown[];
		extra?: Record<string, unknown>;
		/** ISO 8601 timestamp or relative SQL like `NOW() - INTERVAL '1 hour'`.
		 * Defaults to NOW(). */
		createdAt?: string;
	},
): Promise<SeededStatus> {
	const id = randomUUID();
	const useSqlExpr =
		opts.createdAt !== undefined && opts.createdAt.toUpperCase().startsWith("NOW");
	const createdAtClause = opts.createdAt === undefined
		? "NOW()"
		: useSqlExpr
			? opts.createdAt
			: "$8";
	const params: unknown[] = [
		id,
		opts.serverId,
		opts.deviceId ?? null,
		opts.version ?? null,
		opts.healthy ?? true,
		JSON.stringify(opts.health ?? []),
		JSON.stringify(opts.extra ?? {}),
	];
	if (!useSqlExpr && opts.createdAt !== undefined) params.push(opts.createdAt);
	const rows = await sql.query<{ created_at: string }>(
		`INSERT INTO statuses
		 (id, server_id, device_id, version, healthy, health, extra, created_at)
		 VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb, ${createdAtClause})
		 RETURNING created_at`,
		params,
	);
	return { id, createdAt: String(rows[0]!.created_at) };
}

/** Cache row for a Tailscale user's display info, as the auth layer
 * would have upserted it. Used to test avatar/name enrichment. */
export async function seedTailscaleUser(
	sql: Sql,
	opts: { login: string; name?: string; profilePic?: string | null },
): Promise<void> {
	await sql.query(
		`INSERT INTO tailscale_users (login, name, profile_pic) VALUES ($1, $2, $3)`,
		[
			opts.login,
			opts.name ?? opts.login.split("@")[0]!,
			opts.profilePic ?? null,
		],
	);
}

export interface SeededIssue {
	id: string;
}

export async function seedIssue(
	sql: Sql,
	opts: {
		/** Server-scoped issue. Mutually exclusive with `serverGroupId` — the
		 * `issues` scope CHECK requires exactly one set. */
		serverId?: string | null;
		/** Group-scoped issue (e.g. a backup issue spanning the group). When set,
		 * leave `serverId` unset so the row satisfies the scope constraint. */
		serverGroupId?: string | null;
		source?: string;
		ref?: string;
		severity?: string;
		message?: string;
		description?: string | null;
		active?: boolean;
		deviceId?: string | null;
		/** Mark the issue resolved. `resolvedBy` null (with `resolved: true`)
		 * is the "retired on its own" case — the healthcheck recovered with
		 * no operator attributed. A login string attributes it to that
		 * operator (seed a matching tailscale_user for the name lookup). */
		resolved?: boolean;
		resolvedBy?: string | null;
		resolvedReason?: string | null;
	},
): Promise<SeededIssue> {
	const id = randomUUID();
	const resolved = opts.resolved ?? false;
	await sql.query(
		`INSERT INTO issues
		 (id, server_id, server_group_id, device_id, source, ref, severity, message, description, active, first_seen, last_seen, resolved_at, resolved_by, resolved_reason)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW(), NOW(), $11, $12, $13)`,
		[
			id,
			opts.serverId ?? null,
			opts.serverGroupId ?? null,
			opts.deviceId ?? null,
			opts.source ?? "status",
			opts.ref ?? "health",
			opts.severity ?? "error",
			opts.message ?? "Issue message",
			opts.description ?? null,
			resolved ? false : (opts.active ?? true),
			resolved ? new Date().toISOString() : null,
			resolved ? (opts.resolvedBy ?? null) : null,
			resolved ? (opts.resolvedReason ?? null) : null,
		],
	);
	return { id };
}

export interface SeededVersion {
	id: string;
	major: number;
	minor: number;
	patch: number;
}

export async function seedVersion(
	sql: Sql,
	opts: {
		major?: number;
		minor?: number;
		patch?: number;
		status?: "draft" | "published" | "deprecated";
		changelog?: string;
	} = {},
): Promise<SeededVersion> {
	const id = randomUUID();
	const major = opts.major ?? 1;
	const minor = opts.minor ?? Math.floor(Math.random() * 1000);
	const patch = opts.patch ?? 0;
	await sql.query(
		`INSERT INTO versions (id, major, minor, patch, status, changelog)
		 VALUES ($1, $2, $3, $4, $5, $6)`,
		[id, major, minor, patch, opts.status ?? "published", opts.changelog ?? ""],
	);
	return { id, major, minor, patch };
}

// ── Backup-credentials seeding ──────────────────────────────────────────────

export type BackupConfigStatus = "provisioning" | "escrow_pending" | "ready";
export type BackupRepoMode = "from_birth" | "passphrase";

/** Seed a `server_group_backup_config` row for a group, optionally with a
 * `(group, tamanu-postgres)` schedule. */
export async function seedServerGroupBackupConfig(
	sql: Sql,
	opts: {
		groupId: string;
		bucket?: string;
		prefix?: string;
		targetRoleArn?: string;
		maintenanceRoleArn?: string;
		region?: string | null;
		repoPasswordRef?: string;
		status?: BackupConfigStatus;
		mode?: BackupRepoMode;
		lastInitError?: string | null;
		/** Seconds; null = manual-only. Omit to skip seeding a schedule row. */
		intervalSeconds?: number | null;
		retention?: Record<string, number>;
	},
): Promise<void> {
	await sql.query(
		`INSERT INTO server_group_backup_config
		 (group_id, bucket, prefix, target_role_arn, maintenance_role_arn, region, repo_password_ref, status, mode, last_init_error)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)`,
		[
			opts.groupId,
			opts.bucket ?? "bes-kopia-e2e",
			opts.prefix ?? "",
			opts.targetRoleArn ?? "arn:aws:iam::123456789012:role/e2e",
			opts.maintenanceRoleArn ?? "arn:aws:iam::123456789012:role/e2e-maint",
			opts.region ?? null,
			opts.repoPasswordRef ?? `backup-repo-${opts.groupId}`,
			opts.status ?? "ready",
			opts.mode ?? "from_birth",
			opts.lastInitError ?? null,
		],
	);
	if (opts.intervalSeconds !== undefined || opts.retention !== undefined) {
		const retention = opts.retention ?? {
			keep_latest: 1,
			keep_daily: 7,
			keep_weekly: 4,
			keep_monthly: 6,
			keep_annual: 0,
		};
		if (opts.intervalSeconds == null) {
			await sql.query(
				`INSERT INTO server_group_backup_schedule (group_id, type, expected_interval, retention)
				 VALUES ($1, 'tamanu-postgres', NULL, $2::jsonb)`,
				[opts.groupId, JSON.stringify(retention)],
			);
		} else {
			await sql.query(
				`INSERT INTO server_group_backup_schedule (group_id, type, expected_interval, retention)
				 VALUES ($1, 'tamanu-postgres', make_interval(secs => $2), $3::jsonb)`,
				[opts.groupId, opts.intervalSeconds, JSON.stringify(retention)],
			);
		}
	}
}

/** Seed a reported `backup_runs` row. */
export async function seedBackupRun(
	sql: Sql,
	opts: {
		deviceId: string;
		groupId: string;
		serverId?: string | null;
		type?: string;
		purpose?: "backup" | "restore";
		outcome?: "success" | "failure";
		error?: string | null;
		bytesUploaded?: number | null;
		snapshotId?: string | null;
	},
): Promise<{ id: string }> {
	const id = randomUUID();
	await sql.query(
		`INSERT INTO backup_runs
		 (id, device_id, group_id, server_id, type, purpose, outcome, error, bytes_uploaded, snapshot_id)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)`,
		[
			id,
			opts.deviceId,
			opts.groupId,
			opts.serverId ?? null,
			opts.type ?? "tamanu-postgres",
			opts.purpose ?? "backup",
			opts.outcome ?? "success",
			opts.error ?? null,
			opts.bytesUploaded ?? null,
			opts.snapshotId ?? null,
		],
	);
	return { id };
}

/** Seed the cached `backup_repo_stats` row for a group. */
export async function seedBackupRepoStats(
	sql: Sql,
	opts: {
		groupId: string;
		snapshotCount?: number | null;
		sourceCount?: number | null;
		logicalBytes?: number | null;
		physicalBytes?: number | null;
		bucketBytes?: number | null;
	},
): Promise<void> {
	await sql.query(
		`INSERT INTO backup_repo_stats
		 (group_id, snapshot_count, source_count, logical_bytes, physical_bytes, bucket_bytes)
		 VALUES ($1, $2, $3, $4, $5, $6)`,
		[
			opts.groupId,
			opts.snapshotCount ?? null,
			opts.sourceCount ?? null,
			opts.logicalBytes ?? null,
			opts.physicalBytes ?? null,
			opts.bucketBytes ?? null,
		],
	);
}

/** Seed a `server_backup_capabilities` row (what a server advertises it can
 * back up, plus the operator-set enabled flag). */
export async function seedServerBackupCapability(
	sql: Sql,
	opts: {
		serverId: string;
		type?: string;
		enabled?: boolean;
	},
): Promise<void> {
	await sql.query(
		`INSERT INTO server_backup_capabilities (server_id, type, enabled)
		 VALUES ($1, $2, $3)`,
		[opts.serverId, opts.type ?? "tamanu-postgres", opts.enabled ?? true],
	);
}

/** Seed a pending `backup_requests` row (one-off "backup now"). */
export async function seedBackupRequest(
	sql: Sql,
	opts: {
		serverId: string;
		type?: string;
		purpose?: "backup" | "restore";
		requestedBy?: string | null;
	},
): Promise<void> {
	await sql.query(
		`INSERT INTO backup_requests (server_id, type, purpose, requested_by)
		 VALUES ($1, $2, $3, $4)`,
		[
			opts.serverId,
			opts.type ?? "tamanu-postgres",
			opts.purpose ?? "backup",
			opts.requestedBy ?? null,
		],
	);
}
