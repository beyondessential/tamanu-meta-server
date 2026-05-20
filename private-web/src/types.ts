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
// Tuples need separate handling so `[ShortStatus, ServerInfo]` doesn't collapse
// into `(ShortStatus | ServerInfo)[]`. `number extends T['length']` is true for
// regular arrays and false for fixed-length tuples.
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
export type FacilityServerStatus = Solidify<Schemas["FacilityServerStatus"]>;
export type CentralServerCard = Solidify<Schemas["CentralServerCard"]>;
export type SummaryData = Solidify<Schemas["SummaryData"]>;

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

export type DeviceData = Solidify<Schemas["DeviceData"]>;
export type DeviceKeyInfo = Solidify<Schemas["DeviceKeyInfo"]>;
export type DeviceConnectionData = Solidify<Schemas["DeviceConnectionData"]>;
export type DeviceInfo = Solidify<Schemas["DeviceInfo"]>;
export type TailnetLiveInfo = Solidify<Schemas["TailnetLiveInfo"]>;

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
