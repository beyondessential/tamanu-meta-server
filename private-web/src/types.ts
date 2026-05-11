// JSON wire types matching the Rust serde shapes in commons-types.
// Hand-written for now; codegen is a possible future cleanup.

/** Standard wrapper for paginated list responses. */
export interface Page<T> {
	items: T[];
	total: number;
}

export type ShortStatus = "up" | "down" | "away" | "blip" | "gone";

export type ServerKind = "central" | "facility" | "canopy";

export type ServerRank = "production" | "clone" | "demo" | "test" | "dev";

export const SERVER_RANK_ORDER: ServerRank[] = [
	"production",
	"clone",
	"demo",
	"test",
	"dev",
];

/** A semver string like "2.10.5". */
export type VersionStr = string;

export interface FacilityServerStatus {
	id: string;
	name: string;
	up: ShortStatus;
}

export interface CentralServerCard {
	id: string;
	name: string;
	rank: ServerRank | null;
	host: string;
	up: ShortStatus;
	version: VersionStr | null;
	version_distance: number | null;
	facility_servers: FacilityServerStatus[];
}

export interface SummaryData {
	bracket: { min: VersionStr; max: VersionStr };
	releases: Array<[number, number]>;
	versions: VersionStr[];
}

export type ServerGroupedIds = Partial<Record<ServerRank, string[]>>;

export type VersionStatus = "draft" | "published" | "yanked";

export interface VersionData {
	major: number;
	minor: number;
	patch: number;
	status: VersionStatus;
	created_at: string; // RFC 3339 timestamp
}

export interface MinorVersionGroup {
	major: number;
	minor: number;
	count: number;
	latest_patch: number;
	first_created_at: string;
	last_created_at: string;
	versions: VersionData[];
}

export interface RelatedVersionData {
	major: number;
	minor: number;
	patch: number;
	changelog: string;
}

export interface VersionDetail {
	id: string;
	major: number;
	minor: number;
	patch: number;
	status: VersionStatus;
	created_at: string;
	updated_at: string;
	changelog: string;
	min_chrome_version: number | null;
	is_latest_in_minor: boolean;
	related_versions: RelatedVersionData[];
}

export interface ArtifactData {
	id: string;
	artifact_type: string;
	platform: string;
	download_url: string;
	is_exact: boolean;
	version_range_pattern: string | null;
	has_range_override: boolean;
	is_used_in_public_api: boolean;
}

export interface GeoPoint {
	lat: number;
	lon: number;
}

export interface ServerInfoFull {
	id: string;
	name: string | null;
	kind: ServerKind;
	rank: ServerRank | null;
	host: string;
	device_id: string | null;
	parent_server_id: string | null;
	parent_server_name: string | null;
	listed: boolean;
	cloud: boolean | null;
	geolocation: GeoPoint | null;
}

export interface ServerLastStatusData {
	id: string;
	created_at: string;
	version: VersionStr | null;
	version_distance: number | null;
	min_chrome_version: number | null;
	platform: string | null;
	postgres: string | null;
	nodejs: string | null;
	timezone: string | null;
	extra: Record<string, unknown>;
}

export type DeviceRole = "untrusted" | "server" | "releaser" | "admin";

export interface DeviceData {
	id: string;
	created_at: string;
	updated_at: string;
	role: DeviceRole;
}

export interface DeviceKeyInfo {
	id: string;
	device_id: string;
	name: string | null;
	pem_data: string;
	created_at: string;
}

export interface DeviceConnectionData {
	id: string;
	created_at: string;
	device_id: string;
	ip: string;
	user_agent: string | null;
}

export interface DeviceInfoData {
	device: DeviceData;
	keys: DeviceKeyInfo[];
	latest_connection: DeviceConnectionData | null;
}

export interface DeviceShortInfo {
	device: { id: string; role: string };
	keys: Array<{ id: string; name: string | null }>;
	latest_connection: {
		ip: string;
		user_agent: string | null;
	} | null;
}

export interface ServerDetailData {
	server: ServerInfoFull;
	device_info: DeviceShortInfo | null;
	last_status: ServerLastStatusData | null;
	up: ShortStatus;
	child_servers: Array<[ShortStatus, ServerInfoFull]>;
}

/** RFC 5424 syslog severities; enforced server-side. */
export type Severity =
	| "emergency"
	| "alert"
	| "critical"
	| "error"
	| "warning"
	| "notice"
	| "info"
	| "debug";

export const SEVERITIES: Severity[] = [
	"emergency",
	"alert",
	"critical",
	"error",
	"warning",
	"notice",
	"info",
	"debug",
];

/** Reason a human gave when resolving an issue/incident. */
export type ResolvedReason =
	| "fixed"
	| "wont_fix"
	| "expected"
	| "duplicate"
	| "flapping";

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

/** Issue: deduplicated long-lived state of a (server, source, ref) triple. */
export interface IssueData {
	id: string;
	server_id: string;
	/** Display name of the issue's server (may be null — use `server_host`). */
	server_name: string | null;
	server_host: string;
	device_id: string | null;
	source: string;
	ref: string;
	severity: Severity;
	description: string | null;
	message: string;
	active: boolean;
	first_seen: string;
	last_seen: string;
	acknowledged_at: string | null;
	acknowledged_by: string | null;
	acknowledged_by_name: string | null;
	acknowledged_by_pic: string | null;
	resolved_at: string | null;
	resolved_by: string | null;
	resolved_by_name: string | null;
	resolved_by_pic: string | null;
	/** Raw stored value; matches a `ResolvedReason` when set by the API. */
	resolved_reason: string | null;
	snoozed_until: string | null;
	created_at: string;
	updated_at: string;
}

/** Event: a single push, with hybrid coalescing on identical content. */
export interface EventData {
	id: string;
	issue_id: string;
	created_at: string;
	occurred_at: string | null;
	severity: Severity;
	description: string | null;
	message: string;
	active: boolean;
	occurrences: number;
	last_seen: string;
}

/** Incident: server-group rollup; closes when no issue is still active. */
export interface IncidentData {
	id: string;
	server_id: string;
	/** Display name of the root server (may be null — use `server_host`). */
	server_name: string | null;
	server_host: string;
	opened_at: string;
	closed_at: string | null;
	acknowledged_at: string | null;
	acknowledged_by: string | null;
	acknowledged_by_name: string | null;
	acknowledged_by_pic: string | null;
	resolved_at: string | null;
	resolved_by: string | null;
	resolved_by_name: string | null;
	resolved_by_pic: string | null;
	resolved_reason: string | null;
	issue_count: number;
	event_count: number;
	/** Combined: incident notes + notes on all contributing issues. */
	note_count: number;
	created_at: string;
	updated_at: string;
}

export interface IncidentIssueData {
	joined_at: string;
	left_at: string | null;
	issue: IssueData;
}

export interface IncidentWithIssues {
	incident: IncidentData;
	issues: IncidentIssueData[];
}

/** Free-text operator note attached to an issue. Immutable once written. */
export interface IssueNoteData {
	id: string;
	issue_id: string;
	author: string;
	body: string;
	created_at: string;
}

/** Free-text operator note attached to an incident. Immutable once written. */
export interface IncidentNoteData {
	id: string;
	incident_id: string;
	author: string;
	body: string;
	created_at: string;
}

export interface BestoolSnippetInfo {
	id: string;
	name: string;
	description: string | null;
}

export interface BestoolSnippetDetail {
	id: string;
	name: string;
	description: string | null;
	sql: string;
	editor: string;
}

export interface SqlResult {
	columns: string[];
	rows: unknown[][];
	row_count: number;
	execution_time_ms: number;
}

export interface SqlHistoryEntry {
	id: string;
	query: string;
	tailscale_user: string;
	created_at: string;
}
