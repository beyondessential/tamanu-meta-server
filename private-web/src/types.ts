// Wire types for the private-server API.
//
// The schemas in this file are *generated* from the rust handler annotations
// (utoipa) → `private-web/openapi.json` → `api-types.ts`. Regenerate with
// `just gen-openapi` after changing any Rust handler's request or response.
//
// UI-only types and constants (display order, label maps, etc.) stay
// hand-written below the re-exports.

import type { components, paths } from "./api-types";

type Schemas = components["schemas"];

// ── Path-based typing for `callApi` ────────────────────────────────────────
//
// `openapi-typescript` emits a `paths` interface keyed by the OpenAPI path
// strings (e.g. `/api/admins/list`). These helpers project that into the
// `(module, fn)` pair the React hooks have always used, and pull the post
// operation's request body and 200 response type so consumers can stop
// hand-writing them.

export type ApiPath = keyof paths & string;
type ModuleOf<P extends string> = P extends `/api/${infer M}/${string}`
	? M
	: never;
type FnOf<P extends string> = P extends `/api/${string}/${infer F}`
	? F
	: never;

export type ApiModule = ModuleOf<ApiPath>;
export type ApiFn<M extends ApiModule> = {
	[P in ApiPath]: ModuleOf<P> extends M ? FnOf<P> : never;
}[ApiPath];

export type ApiPathFor<M extends ApiModule, F extends ApiFn<M>> =
	`/api/${M}/${F}` extends ApiPath ? `/api/${M}/${F}` : never;

type PostOp<P extends ApiPath> = paths[P]["post"];

export type ApiResponse<M extends ApiModule, F extends ApiFn<M>> =
	PostOp<ApiPathFor<M, F>> extends {
		responses: { 200: { content: { "application/json": infer R } } };
	}
		? Solidify<R>
		: void;

export type ApiBody<M extends ApiModule, F extends ApiFn<M>> =
	PostOp<ApiPathFor<M, F>> extends {
		requestBody: { content: { "application/json": infer B } };
	}
		? Solidify<B>
		: Record<string, unknown> | undefined;

// utoipa marks `Option<T>` Rust fields as not-required AND nullable, so
// `openapi-typescript` emits them as `field?: T | null`. But serde's default
// for `Option<T>` is to always emit the field (as `null` for None), so the
// optional `?` is wrong at runtime — every field is present. `Solidify` peels
// that off, making `field: T | null`, which is what the wire shape actually
// gives us.
//
// Tuples need separate handling so `[u64, u64]` doesn't collapse into
// `(u64 | u64)[]`. `number extends T['length']` is true for regular arrays
// and false for fixed-length tuples.
export type Solidify<T> = T extends readonly unknown[]
	? number extends T["length"]
		? Solidify<T[number]>[]
		: { [K in keyof T]: Solidify<Exclude<T[K], undefined>> }
	: T extends object
		? { [K in keyof T]-?: Solidify<Exclude<T[K], undefined>> }
		: T;

// ── Wire types ─────────────────────────────────────────────────────────────

export type ShortStatus = Solidify<Schemas["ShortStatus"]>;
export type HealthState = Solidify<Schemas["HealthState"]>;
export type ServerKind = Solidify<Schemas["ServerKind"]>;
export type ServerRank = Solidify<Schemas["ServerRank"]>;
export type VersionStatus = Solidify<Schemas["VersionStatus"]>;
export type DeviceRole = Solidify<Schemas["DeviceRole"]>;
export type Severity = Solidify<Schemas["Severity"]>;
export type ResolvedReason = Solidify<Schemas["ResolvedReason"]>;

export type VersionStr = Solidify<Schemas["VersionStr"]>;

export type GeoPoint = Solidify<Schemas["GeoPoint"]>;
export type OperatorPresence = Solidify<Schemas["OperatorPresence"]>;
export type FacilityServerStatus = Solidify<Schemas["FacilityServerStatus"]>;
export type ServerGroupCard = Solidify<Schemas["ServerGroupCard"]>;
export type ServerGroup = Solidify<Schemas["ServerGroup"]>;
export type GroupDetail = Solidify<Schemas["GroupDetail"]>;
export type SummaryData = Solidify<Schemas["SummaryData"]>;
export type TagMap = Solidify<Schemas["TagMap"]>;

export type VersionData = Solidify<Schemas["VersionData"]>;
export type MinorVersionGroup = Solidify<Schemas["MinorVersionGroup"]>;
export type RelatedVersionData = Solidify<Schemas["RelatedVersionData"]>;
export type VersionDetail = Solidify<Schemas["VersionDetail"]>;
export type KnownIssueData = Solidify<Schemas["KnownIssueData"]>;
export type ArtifactData = Solidify<Schemas["ArtifactData"]>;

export type ServerInfo = Solidify<Schemas["ServerInfo"]>;
export type ServerLastStatusData = Solidify<Schemas["ServerLastStatusData"]>;
export type ServerDetailData = Solidify<Schemas["ServerDetailData"]>;
export type StatusSnapshotData = Solidify<Schemas["StatusSnapshotData"]>;
export type ServerSilencedRef = Solidify<Schemas["ServerSilencedRef"]>;
export type ServerGroupSilencedRef = Solidify<Schemas["ServerGroupSilencedRef"]>;

export type EnrollmentTicket = Solidify<Schemas["EnrollmentTicket"]>;
export type EnrollmentStatus = Solidify<Schemas["EnrollmentStatus"]>;

export type DeviceData = Solidify<Schemas["DeviceData"]>;
export type DeviceKeyInfo = Solidify<Schemas["DeviceKeyInfo"]>;
export type DeviceConnectionData = Solidify<Schemas["DeviceConnectionData"]>;
export type DeviceInfo = Solidify<Schemas["DeviceInfo"]>;
export type TailnetLiveInfo = Solidify<Schemas["TailnetLiveInfo"]>;

export type HealthcheckSeverityData = Solidify<Schemas["HealthcheckSeverityData"]>;
export type HealthcheckSample = Solidify<Schemas["HealthcheckSample"]>;
export type HealthcheckSampleResponse = Solidify<Schemas["HealthcheckSampleResponse"]>;

export type IssueData = Solidify<Schemas["IssueData"]>;
export type IssueIncidentLink = Solidify<Schemas["IssueIncidentLink"]>;
export type EventData = Solidify<Schemas["EventData"]>;
export type IncidentData = Solidify<Schemas["IncidentData"]>;
export type IncidentIssueData = Solidify<Schemas["IncidentIssueData"]>;
export type IncidentWithIssues = Solidify<Schemas["IncidentWithIssues"]>;
export type IssueNoteData = Solidify<Schemas["IssueNoteData"]>;
export type IncidentNoteData = Solidify<Schemas["IncidentNoteData"]>;

export type BestoolSnippetInfo = Solidify<Schemas["BestoolSnippetInfo"]>;
export type BestoolSnippetDetail = Solidify<Schemas["BestoolSnippetDetail"]>;

export type SqlResult = Solidify<Schemas["SqlResult"]>;
export type SqlHistoryEntry = Solidify<Schemas["SqlHistoryEntry"]>;

// ── Pagination wrapper ─────────────────────────────────────────────────────
//
// utoipa emits one schema per concrete `Page<T>` instantiation
// (`Page_DeviceInfo`, `Page_ServerInfo`, …) — there's no parametric `Page<T>`
// in the generated file. We keep a hand-written generic here so call sites
// (mostly state shapes outside `useApi`) can still write `Page<DeviceInfo>`;
// it's structurally identical to each emitted variant.
export interface Page<T> {
	items: T[];
	total: number;
}

// ── UI-only types ──────────────────────────────────────────────────────────

export type ServerGroupedIds = Partial<Record<ServerRank, string[]>>;

// ── UI-only display order / labels ─────────────────────────────────────────

export const SERVER_RANK_ORDER: ServerRank[] = [
	"production",
	"clone",
	"demo",
	"test",
	"dev",
];

/// Display order for server kinds — centrals first, then facilities,
/// then canopy's own. Used as a tiebreak within a single rank in
/// status-dot lists / group detail views.
export const SERVER_KIND_ORDER: ServerKind[] = ["central", "facility", "canopy"];

/// Sort key combining rank index (with `null` ranks pushed last) and
/// kind index. Stable per-rank ordering matches what the UI grouping
/// expects.
export function serverSortKey(s: {
	rank?: ServerRank | null;
	kind: ServerKind;
}): [number, number] {
	const rankIndex =
		s.rank == null ? Infinity : SERVER_RANK_ORDER.indexOf(s.rank);
	const rankKey = rankIndex === -1 ? SERVER_RANK_ORDER.length : rankIndex;
	const kindIndex = SERVER_KIND_ORDER.indexOf(s.kind);
	const kindKey = kindIndex === -1 ? SERVER_KIND_ORDER.length : kindIndex;
	return [rankKey, kindKey];
}

export function compareServersByRankThenKind<
	T extends { rank?: ServerRank | null; kind: ServerKind; name?: string | null },
>(a: T, b: T): number {
	const [ar, ak] = serverSortKey(a);
	const [br, bk] = serverSortKey(b);
	if (ar !== br) return ar - br;
	if (ak !== bk) return ak - bk;
	const an = a.name ?? "";
	const bn = b.name ?? "";
	return an.localeCompare(bn);
}

/// Operator-facing severity vocabulary, loud → quiet. Used for both
/// display and selection (the API now restricts severities to these
/// five — see commons-types::issue::Severity and the
/// 2026-05-29-restrict_severities migration).
export const SEVERITIES: Severity[] = [
	"critical",
	"error",
	"warning",
	"info",
	"debug",
];

/// Short one-line description of how each severity participates in the
/// incident workflow. Used in dropdown helper text and as the
/// SeverityChip tooltip so operators see the semantic meaning at the
/// point of choice.
export const SEVERITY_INTENT: Record<Severity, string> = {
	critical: "Opens an incident immediately (no holding period)",
	error: "Opens an incident (after the group's holding period)",
	warning: "Joins an open incident; doesn't open one on its own",
	info: "Joins an open incident; doesn't open one on its own",
	debug: "Not shown in incidents",
};

/// Per-check result vocabulary. Hand-written mirror of the Rust
/// `commons_types::status::CheckResult` (the source of truth) — the
/// private API ships `health[]` as raw JSON, so this never appears in
/// the generated schema. Also the UI display order: most to least
/// urgent.
export type CheckResult =
	| "failed"
	| "warning"
	| "broken"
	| "skipped"
	| "passed";

export const CHECK_RESULT_ORDER: CheckResult[] = [
	"failed",
	"warning",
	"broken",
	"skipped",
	"passed",
];

/// Normalise a raw `health[]` entry to its result. Mirror of the Rust
/// `CheckResult::from_entry`: prefer a valid `result` string (an
/// unknown string is null, NOT reinterpreted via `healthy`), else the
/// legacy `healthy: bool` (true → passed, false → failed), else null
/// (malformed entry, callers skip it).
export function checkResultOf(
	entry: Record<string, unknown>,
): CheckResult | null {
	const result = entry.result;
	if (typeof result === "string") {
		return (CHECK_RESULT_ORDER as string[]).includes(result)
			? (result as CheckResult)
			: null;
	}
	const healthy = entry.healthy;
	if (typeof healthy === "boolean") return healthy ? "passed" : "failed";
	return null;
}

/// One person connected somewhere in a server group, with the names of
/// the member servers they're on. Produced by [`aggregateOperators`].
export type AggregatedOperator = {
	op: OperatorPresence;
	servers: string[];
};

/// Aggregate per-member operator presence into distinct people across the
/// group: deduped by login, keeping the earliest `connected_since` and
/// collecting which member servers each person is connected to. Shared by
/// the status-page group cards and the group detail page so both show the
/// same numbers.
export function aggregateOperators(
	members: FacilityServerStatus[],
): AggregatedOperator[] {
	const byLogin = new Map<string, AggregatedOperator>();
	for (const m of members) {
		for (const op of m.operators) {
			const serverName = m.name || "(unnamed)";
			const existing = byLogin.get(op.login);
			if (!existing) {
				byLogin.set(op.login, { op, servers: [serverName] });
				continue;
			}
			if (!existing.servers.includes(serverName)) {
				existing.servers.push(serverName);
			}
			if (
				op.connected_since &&
				(!existing.op.connected_since ||
					Date.parse(op.connected_since) <
						Date.parse(existing.op.connected_since))
			) {
				existing.op = { ...existing.op, connected_since: op.connected_since };
			}
		}
	}
	return [...byLogin.values()];
}

export const RESOLVED_REASONS: ResolvedReason[] = [
	"fixed",
	"wont_fix",
	"expected",
	"duplicate",
	"flapping",
];

export const RESOLVED_REASON_LABEL: Record<ResolvedReason, string> = {
	fixed: "Fixed",
	wont_fix: "Won't fix",
	expected: "Expected",
	duplicate: "Duplicate",
	flapping: "Flapping",
};

// Shown wherever a resolver is named but no operator login is attached: the
// incident/issue retired because its healthcheck started reporting healthy
// again. Phrased as the triggering event, not an actor — it describes what
// canopy observed, and does not imply that nobody intervened. Keep in sync
// with the Slack `incident_resolve` default in
// crates/database/src/slack_outbox/vars.rs.
export const AUTOMATION_RESOLVER_LABEL = "the healthcheck recovering";
