import type { Page } from "@playwright/test";
import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedIncident,
	seedIncidentNote,
	seedIssue,
	seedServer,
	seedServerGroup,
} from "./seed";

// An incident's timeline leads with what is failing. Ordering is severity
// first, most recent second, and notes sit below every issue — an operator
// opening a page mid-incident shouldn't have to read a chronology to find
// the failure. Each fixture below is seeded so that ordering by time alone
// would produce the opposite order, so a passing assertion can only mean the
// severity rank is being applied.
test.describe("incident timeline ordering", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	async function seedTimeline(sql: Parameters<typeof seedIssue>[0]) {
		const group = await seedServerGroup(sql, { name: "ordering-group" });
		const server = await seedServer(sql, {
			name: "ordering-server",
			kind: "central",
			rank: "production",
			groupId: group.id,
		});

		const minutesAgo = (n: number) =>
			new Date(Date.now() - n * 60_000).toISOString();

		// Assertions key off the message, which is the row's always-visible
		// summary text. The check name only shows in the expanded provenance
		// line, and a recovered issue renders collapsed.
		//
		// Deliberately inverted: the newest thing is the least severe, so
		// time-ordering alone would put the warning above the failure.
		const warning = await seedIssue(sql, {
			serverId: server.id,
			ref: "health/freshwarning",
			severity: "warning",
			message: "marker-warning-newest",
		});
		const failure = await seedIssue(sql, {
			serverId: server.id,
			ref: "health/oldfailure",
			severity: "error",
			message: "marker-failure-oldest",
		});
		const recovered = await seedIssue(sql, {
			serverId: server.id,
			ref: "health/middlerecovered",
			active: false,
			message: "marker-recovered-middle",
		});

		const incident = await seedIncident(sql, {
			serverGroupId: group.id,
			openedAt: minutesAgo(60),
			issues: [
				{ issueId: warning.id, joinedAt: minutesAgo(5) },
				{ issueId: recovered.id, joinedAt: minutesAgo(30) },
				{ issueId: failure.id, joinedAt: minutesAgo(60) },
			],
		});
		// Newest entry on the page, and still below every issue.
		await seedIncidentNote(sql, {
			incidentId: incident.id,
			body: "notewrittenlast",
			createdAt: minutesAgo(1),
		});
		return { group, server, incident };
	}

	/** Where each marker first appears in the rendered page text. Rows render
	 * in order, so a marker unique to one row first appears inside it, and
	 * comparing offsets compares row positions. */
	async function positionsOf(page: Page, markers: string[]): Promise<number[]> {
		const body = await page.locator("body").innerText();
		return markers.map((marker) => {
			const at = body.indexOf(marker);
			expect(at, `${marker} should be on the page`).toBeGreaterThan(-1);
			return at;
		});
	}

	test("failures sort above warnings, recoveries and notes", async ({
		page,
		sql,
	}) => {
		const { incident } = await seedTimeline(sql);

		await page.goto(`/incidents/${incident.id}`);
		await expect(page.getByText("marker-failure-oldest").first()).toBeVisible();

		const [failure, warning, recovered, note] = await positionsOf(page, [
			"marker-failure-oldest",
			"marker-warning-newest",
			"marker-recovered-middle",
			"notewrittenlast",
		]);

		expect(failure, "the failure leads, despite being the oldest").toBeLessThan(
			warning,
		);
		expect(warning, "the warning outranks the recovered check").toBeLessThan(
			recovered,
		);
		expect(recovered, "notes sit below every issue").toBeLessThan(note);
	});

	test("the issues filter keeps the same order", async ({ page, sql }) => {
		const { incident } = await seedTimeline(sql);

		await page.goto(`/incidents/${incident.id}`);
		await page.getByText(/^Issues \(/).click();
		await expect(page.getByText("marker-failure-oldest").first()).toBeVisible();

		const [failure, warning] = await positionsOf(page, [
			"marker-failure-oldest",
			"marker-warning-newest",
		]);
		expect(failure).toBeLessThan(warning);
		await expect(page.getByText("notewrittenlast")).toHaveCount(0);
	});
});
