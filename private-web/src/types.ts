// JSON wire types matching the Rust serde shapes in commons-types.
// Hand-written for now; codegen is a possible future cleanup.

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
