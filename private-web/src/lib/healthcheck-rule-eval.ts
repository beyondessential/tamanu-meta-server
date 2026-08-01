// Client-side mirror of the Rust IfLadder evaluator (see
// crates/database/src/check_policies.rs). Used by the rule
// editor to preview whether a candidate rule would match a sampled
// status push.
//
// The Rust side is the source of truth. Behaviours that must stay in
// sync:
//   - Missing field → condition false (not an error).
//   - `==` / `!=` compare strict-equal first; numeric-string and
//     number compare numerically when both sides are numeric-coercible.
//   - `<` / `<=` / `>` / `>=` are numeric; non-numeric → false.
//   - `in_range` parses lhs as a semver Version, value as a semver Range.

import semver from "semver";
import type { HealthcheckSample } from "../types";

export type RuleOp = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in_range";

export interface Condition {
	varPath: string;
	op: RuleOp;
	value: unknown;
}

export interface PreviewResult {
	/** True if the var path resolves in the sample. */
	varResolved: boolean;
	/** The resolved lhs value (when varResolved). */
	lhs: unknown;
	/** True if the condition matches the sample. */
	matched: boolean;
	/** Per-piece diagnostics — what we tried, why we said yes/no. */
	notes: string;
}

function resolveVar(varPath: string, sample: HealthcheckSample): { found: boolean; value: unknown } {
	const dot = varPath.indexOf(".");
	if (dot <= 0) return { found: false, value: undefined };
	const kind = varPath.slice(0, dot);
	const field = varPath.slice(dot + 1);
	let map: Record<string, unknown>;
	if (kind === "check") map = sample.check_extra;
	else if (kind === "status") map = sample.status_extra;
	else if (kind === "tag") map = sample.tags;
	else return { found: false, value: undefined };
	if (!(field in map)) return { found: false, value: undefined };
	return { found: true, value: map[field] };
}

/**
 * What Rust's `str::parse::<f64>()` accepts, which is narrower than
 * `Number()`: no hex/binary/octal (`0x10` is 16 to JS, an error to Rust), no
 * digit separators, and no surrounding whitespace.
 *
 * `inf`/`nan` spellings are left out. Rust accepts them, but they can only
 * arrive as strings, and a string equal to another is already structurally
 * equal — while `inf` compared against any real number is false either way.
 */
const RUST_F64 = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;

function toNumber(v: unknown): number | null {
	if (typeof v === "number") return Number.isFinite(v) ? v : null;
	if (typeof v !== "string") return null;
	// Deliberately not trimmed: `" 12 ".parse::<f64>()` is an error in Rust.
	if (!RUST_F64.test(v)) return null;
	const n = Number(v);
	return Number.isFinite(n) ? n : null;
}

/**
 * Structural equality, mirroring `PartialEq for serde_json::Value` — Rust
 * compares arrays element-wise and objects key-wise, so `[1,2] == [1,2]` is
 * true there. `===` is reference equality for both, which made the preview
 * say the opposite of what the rule does.
 */
function deepEqual(a: unknown, b: unknown): boolean {
	if (a === b) return true;
	if (a === null || b === null) return false;
	if (typeof a !== "object" || typeof b !== "object") return false;
	if (Array.isArray(a) !== Array.isArray(b)) return false;
	if (Array.isArray(a) && Array.isArray(b)) {
		return a.length === b.length && a.every((x, i) => deepEqual(x, b[i]));
	}
	const ao = a as Record<string, unknown>;
	const bo = b as Record<string, unknown>;
	const ak = Object.keys(ao);
	// Key order is irrelevant to `serde_json::Value` equality, so compare as
	// sets of entries rather than in insertion order.
	return (
		ak.length === Object.keys(bo).length &&
		ak.every((k) => k in bo && deepEqual(ao[k], bo[k]))
	);
}

function jsonEqual(a: unknown, b: unknown): boolean {
	if (deepEqual(a, b)) return true;
	// Numeric coercion: same as Rust evaluator.
	const an = toNumber(a);
	const bn = toNumber(b);
	if (an !== null && bn !== null) return an === bn;
	return false;
}

export function evaluate(condition: Condition, sample: HealthcheckSample): PreviewResult {
	const { varPath, op, value } = condition;
	const { found, value: lhs } = resolveVar(varPath, sample);
	if (!found) {
		return {
			varResolved: false,
			lhs: undefined,
			matched: false,
			notes: `${varPath} is not present in the sample, so the condition evaluates to false.`,
		};
	}
	const fmt = (v: unknown) =>
		typeof v === "string" ? JSON.stringify(v) : JSON.stringify(v);
	switch (op) {
		case "==": {
			const matched = jsonEqual(lhs, value);
			return {
				varResolved: true,
				lhs,
				matched,
				notes: `${fmt(lhs)} ${matched ? "==" : "!="} ${fmt(value)}`,
			};
		}
		case "!=": {
			const matched = !jsonEqual(lhs, value);
			return {
				varResolved: true,
				lhs,
				matched,
				notes: `${fmt(lhs)} ${matched ? "!=" : "=="} ${fmt(value)}`,
			};
		}
		case "<":
		case "<=":
		case ">":
		case ">=": {
			const ln = toNumber(lhs);
			const rn = toNumber(value);
			if (ln === null || rn === null) {
				return {
					varResolved: true,
					lhs,
					matched: false,
					notes: `${fmt(lhs)} or ${fmt(value)} is not numeric — comparison evaluates to false.`,
				};
			}
			const matched =
				op === "<"
					? ln < rn
					: op === "<="
						? ln <= rn
						: op === ">"
							? ln > rn
							: ln >= rn;
			return {
				varResolved: true,
				lhs,
				matched,
				notes: `${ln} ${op} ${rn}`,
			};
		}
		case "in_range": {
			if (typeof lhs !== "string") {
				return {
					varResolved: true,
					lhs,
					matched: false,
					notes: `${fmt(lhs)} is not a string, so it can't be a semver version.`,
				};
			}
			if (typeof value !== "string") {
				return {
					varResolved: true,
					lhs,
					matched: false,
					notes: `value ${fmt(value)} is not a string, so it can't be a semver range.`,
				};
			}
			// Strict parse, no `coerce`. The Rust evaluator this mirrors does
			// `parse::<node_semver::Version>()`, which rejects a partial like
			// "2.28" outright and keeps a prerelease suffix intact. `coerce`
			// fabricates "2.28.0" from the former and discards the suffix on
			// the latter, so the preview would report a match where production
			// files nothing.
			const ver = semver.valid(lhs);
			const range = semver.validRange(value);
			if (!ver) {
				return {
					varResolved: true,
					lhs,
					matched: false,
					notes: `'${lhs}' is not a valid semver version.`,
				};
			}
			if (!range) {
				return {
					varResolved: true,
					lhs,
					matched: false,
					notes: `'${value}' is not a valid semver range.`,
				};
			}
			const matched = semver.satisfies(ver, range);
			return {
				varResolved: true,
				lhs,
				matched,
				notes: matched
					? `${ver} satisfies ${value}.`
					: `${ver} does not satisfy ${value}.`,
			};
		}
	}
}
