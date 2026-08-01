import { describe, expect, it } from "vitest";
import { type Condition, evaluate } from "./healthcheck-rule-eval";
import type { HealthcheckSample } from "../types";

function sample(statusExtra: Record<string, unknown>): HealthcheckSample {
	return {
		check_extra: {},
		seen_at: "2026-01-01T00:00:00Z",
		server_host: "https://example.invalid",
		server_name: "example",
		status_extra: statusExtra,
		tags: {},
	} as HealthcheckSample;
}

function inRange(version: unknown, range: string): boolean {
	const condition: Condition = {
		varPath: "status.tamanuVersion",
		op: "in_range",
		value: range,
	};
	return evaluate(condition, sample({ tamanuVersion: version })).matched;
}

// The Rust evaluator (crates/database/src/check_policies.rs) is the source of
// truth: it parses the left-hand side with `parse::<node_semver::Version>()`,
// which is strict. Coercing here would make the editor's preview promise a
// match that production ingestion never files.
describe("in_range mirrors the Rust evaluator's strict version parse", () => {
	it("does not fabricate a patch for a partial version", () => {
		expect(inRange("2.28", ">=2.28.0")).toBe(false);
		expect(inRange("2", ">=1.0.0")).toBe(false);
	});

	it("keeps a prerelease suffix, so it doesn't satisfy a stable range", () => {
		expect(inRange("2.28.0-rc.1", ">=2.28.0")).toBe(false);
	});

	it("still matches an ordinary version", () => {
		expect(inRange("2.28.0", ">=2.28.0")).toBe(true);
		expect(inRange("2.29.3", "^2.28.0")).toBe(true);
		expect(inRange("2.27.9", ">=2.28.0")).toBe(false);
	});

	it("matches a prerelease against a range that names one", () => {
		expect(inRange("2.28.0-rc.2", ">=2.28.0-rc.1")).toBe(true);
	});

	it("is false for a non-string left-hand side", () => {
		expect(inRange(228, ">=2.28.0")).toBe(false);
	});
});

// The Rust side compares `serde_json::Value`s, which is structural: arrays
// and objects are equal by content. `===` is reference equality for both, so
// the preview reported the opposite of what the rule does.
describe("== / != mirror Rust's structural JSON equality", () => {
	function eq(lhs: unknown, rhs: unknown): boolean {
		return evaluate(
			{ varPath: "status.v", op: "==", value: rhs },
			sample({ v: lhs }),
		).matched;
	}

	it("compares arrays by content", () => {
		expect(eq([1, 2], [1, 2])).toBe(true);
		expect(eq([1, 2], [2, 1])).toBe(false);
		expect(eq([1, 2], [1, 2, 3])).toBe(false);
	});

	it("compares objects by content, regardless of key order", () => {
		expect(eq({ a: 1, b: 2 }, { b: 2, a: 1 })).toBe(true);
		expect(eq({ a: 1 }, { a: 2 })).toBe(false);
		expect(eq({ a: 1 }, { a: 1, b: 2 })).toBe(false);
	});

	it("compares nested structures", () => {
		expect(eq({ a: [1, { b: null }] }, { a: [1, { b: null }] })).toBe(true);
		expect(eq({ a: [1, { b: null }] }, { a: [1, { b: 0 }] })).toBe(false);
	});

	it("does not confuse an array with an object", () => {
		expect(eq([], {})).toBe(false);
	});

	it("!= is the negation, not a separate comparison", () => {
		const matched = evaluate(
			{ varPath: "status.v", op: "!=", value: [1, 2] },
			sample({ v: [1, 2] }),
		).matched;
		expect(matched).toBe(false);
	});
});

// `Number()` is more permissive than Rust's `str::parse::<f64>()`, so a
// string the rule treats as non-numeric could compare numerically here.
describe("numeric coercion matches Rust's f64 parse", () => {
	function gt(lhs: unknown, rhs: unknown): boolean {
		return evaluate(
			{ varPath: "status.v", op: ">", value: rhs },
			sample({ v: lhs }),
		).matched;
	}

	it("still coerces ordinary numeric strings", () => {
		expect(gt("21608625", 100)).toBe(true);
		expect(gt("1.5e3", 1000)).toBe(true);
		expect(gt("-3", -4)).toBe(true);
	});

	it("rejects what Rust rejects", () => {
		// Number("0x10") is 16; "0x10".parse::<f64>() is an error.
		expect(gt("0x10", 1)).toBe(false);
		// Digit separators and surrounding whitespace are Rust errors too.
		expect(gt("1_000", 1)).toBe(false);
		expect(gt(" 12 ", 1)).toBe(false);
		expect(gt("", 1)).toBe(false);
	});
});
