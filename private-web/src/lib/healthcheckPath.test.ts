import { describe, expect, it } from "vitest";
import { healthcheckPath, healthcheckSettingsPath } from "../types";

// Check names are arbitrary strings a device chose, not restricted to
// URL-safe characters. A name carrying `/`, `?`, `#`, or `%` routes to the
// wrong page — or nowhere — unless every link builder encodes it, which is
// what these two helpers exist to guarantee.
describe("healthcheck link builders", () => {
	const awkward: [string, string][] = [
		["a/b", "a%2Fb"],
		["what?", "what%3F"],
		["frag#ment", "frag%23ment"],
		["100%", "100%25"],
		["with space", "with%20space"],
	];

	it.each(awkward)("encodes %j in the affected-servers path", (raw, encoded) => {
		expect(healthcheckPath("alertd", raw)).toBe(`/healthchecks/alertd/${encoded}`);
	});

	it.each(awkward)("encodes %j in the settings path", (raw, encoded) => {
		expect(healthcheckSettingsPath("alertd", raw)).toBe(
			`/settings/healthchecks/alertd/${encoded}`,
		);
	});

	it("encodes the source too", () => {
		expect(healthcheckPath("a/b", "disk")).toBe("/healthchecks/a%2Fb/disk");
		expect(healthcheckSettingsPath("a/b", "disk")).toBe("/settings/healthchecks/a%2Fb/disk");
	});

	it("leaves an ordinary name alone", () => {
		expect(healthcheckPath("alertd", "disk_free")).toBe("/healthchecks/alertd/disk_free");
	});
});
