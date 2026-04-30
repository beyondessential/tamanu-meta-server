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
