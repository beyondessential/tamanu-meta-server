// Wire types for the private-server API.
//
// The schemas in this file are *generated* from the rust handler annotations
// (utoipa) → `private-web/openapi.json` → `api-types.ts`. Regenerate with
// `just gen-openapi` after changing any Rust handler's request or response.
//
// UI-only types and constants (display order, label maps, etc.) stay
// hand-written below the re-exports.

import type { components } from "./api-types";

type Schemas = components["schemas"];

// utoipa marks `Option<T>` Rust fields as not-required AND nullable, so
// `openapi-typescript` emits them as `field?: T | null`. But serde's default
// for `Option<T>` is to always emit the field (as `null` for None), so the
// optional `?` is wrong at runtime — every field is present. `Solidify` peels
// that off, making `field: T | null`, which is what the wire shape actually
// gives us.
//
// Tuples need separate handling so `[ShortStatus, ServerInfo]` doesn't collapse
// into `(ShortStatus | ServerInfo)[]`. `number extends T['length']` is true for
// regular arrays and false for fixed-length tuples.
type Solidify<T> = T extends readonly unknown[]
	? number extends T["length"]
		? Solidify<T[number]>[]
		: { [K in keyof T]: Solidify<Exclude<T[K], undefined>> }
	: T extends object
		? { [K in keyof T]-?: Solidify<Exclude<T[K], undefined>> }
		: T;

// ── Wire types ─────────────────────────────────────────────────────────────

export type ShortStatus = Solidify<Schemas["ShortStatus"]>;
export type ServerKind = Solidify<Schemas["ServerKind"]>;
export type ServerRank = Solidify<Schemas["ServerRank"]>;
export type VersionStatus = Solidify<Schemas["VersionStatus"]>;
export type DeviceRole = Solidify<Schemas["DeviceRole"]>;
export type Severity = Solidify<Schemas["Severity"]>;
export type ResolvedReason = Solidify<Schemas["ResolvedReason"]>;

export type VersionStr = Solidify<Schemas["VersionStr"]>;

export type GeoPoint = Solidify<Schemas["GeoPoint"]>;
export type FacilityServerStatus = Solidify<Schemas["FacilityServerStatus"]>;
export type CentralServerCard = Solidify<Schemas["CentralServerCard"]>;
export type SummaryData = Solidify<Schemas["SummaryData"]>;

export type VersionData = Solidify<Schemas["VersionData"]>;
export type MinorVersionGroup = Solidify<Schemas["MinorVersionGroup"]>;
export type RelatedVersionData = Solidify<Schemas["RelatedVersionData"]>;
export type VersionDetail = Solidify<Schemas["VersionDetail"]>;
export type ArtifactData = Solidify<Schemas["ArtifactData"]>;

export type ServerInfo = Solidify<Schemas["ServerInfo"]>;
export type ServerLastStatusData = Solidify<Schemas["ServerLastStatusData"]>;
export type ServerDetailData = Solidify<Schemas["ServerDetailData"]>;

export type DeviceData = Solidify<Schemas["DeviceData"]>;
export type DeviceKeyInfo = Solidify<Schemas["DeviceKeyInfo"]>;
export type DeviceConnectionData = Solidify<Schemas["DeviceConnectionData"]>;
export type DeviceInfo = Solidify<Schemas["DeviceInfo"]>;

export type IssueData = Solidify<Schemas["IssueData"]>;
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

// Aliases for the hand-written names still used by the React code. These are
// kept so the rename doesn't fan out through every component; new code should
// prefer the unsuffixed names above.
export type DeviceInfoData = DeviceInfo;
export type DeviceShortInfo = DeviceInfo;
export type ServerInfoFull = ServerInfo;

// ── Pagination wrapper ─────────────────────────────────────────────────────
//
// utoipa emits one schema per concrete `Page<T>` instantiation
// (`Page_DeviceInfo`, `Page_ServerInfo`, …) — there's no parametric `Page<T>`
// in the generated file. We keep a hand-written generic here so call sites
// can still write `useApi<Page<DeviceInfo>>(...)`; it's structurally
// identical to each emitted variant.
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
