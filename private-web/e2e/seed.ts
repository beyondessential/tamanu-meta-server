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
		"TRUNCATE statuses, issues, device_keys, servers, server_groups, devices, versions RESTART IDENTITY CASCADE",
	);
}

export interface SeededServerGroup {
	id: string;
	name: string;
}

export async function seedServerGroup(
	sql: Sql,
	opts: { name?: string; notes?: string; tags?: Record<string, string> } = {},
): Promise<SeededServerGroup> {
	const id = randomUUID();
	const name = opts.name ?? randomLabel("group");
	await sql.query(
		`INSERT INTO server_groups (id, name, notes, tags)
		 VALUES ($1, $2, $3, $4::jsonb)`,
		[id, name, opts.notes ?? "", JSON.stringify(opts.tags ?? {})],
	);
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
		/** Threshold in seconds; `0` disables alerting (the default for e2e seeds). */
		alertWhenDownFor?: number;
	} = {},
): Promise<SeededServer> {
	const id = randomUUID();
	const name = opts.name ?? randomLabel("srv");
	const host = opts.host ?? `https://${randomLabel("host")}.e2e.invalid`;
	const kind = opts.kind ?? "central";
	const rank = opts.rank ?? "production";
	const alertWhenDownFor = opts.alertWhenDownFor ?? 0;
	await sql.query(
		`INSERT INTO servers (id, name, host, kind, rank, group_id, device_id, alert_when_down_for, notes, tags)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb)`,
		[
			id,
			name,
			host,
			kind,
			rank,
			opts.groupId ?? null,
			opts.deviceId ?? null,
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
			: "$7";
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

export interface SeededIssue {
	id: string;
}

export async function seedIssue(
	sql: Sql,
	opts: {
		serverId: string;
		source?: string;
		ref?: string;
		severity?: string;
		message?: string;
		description?: string | null;
		active?: boolean;
		deviceId?: string | null;
	},
): Promise<SeededIssue> {
	const id = randomUUID();
	await sql.query(
		`INSERT INTO issues
		 (id, server_id, device_id, source, ref, severity, message, description, active, first_seen, last_seen)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())`,
		[
			id,
			opts.serverId,
			opts.deviceId ?? null,
			opts.source ?? "status",
			opts.ref ?? "health",
			opts.severity ?? "error",
			opts.message ?? "Issue message",
			opts.description ?? null,
			opts.active ?? true,
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
