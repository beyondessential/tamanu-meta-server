import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedIncident,
	seedIssue,
	seedMaintenanceWindow,
	seedServer,
	seedServerGroup,
	seedStatus,
	seedUpgradePlan,
	seedVersion,
	type Sql,
} from "./seed";

async function openWindows(sql: Sql): Promise<number> {
	const rows = await sql.query<{ n: string }>(
		"SELECT COUNT(*) AS n FROM maintenance_windows WHERE ended_at IS NULL",
	);
	return Number(rows[0]!.n);
}

// spec: MNT
test.describe("maintenance windows", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("a server under a window says so, and its health is marked", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "under-the-knife" });
		const server = await seedServer(sql, {
			name: "being-upgraded",
			groupId: group.id,
		});
		await seedStatus(sql, { serverId: server.id, healthy: true });
		await seedMaintenanceWindow(sql, {
			serverId: server.id,
			note: "Upgrading to 2.62",
			declaredBy: "daniel@bes.au",
		});

		await page.goto(`/servers/${server.id}`);
		const section = page.getByTestId("maintenance-section");
		await expect(section).toContainText("Under maintenance, ending");
		await expect(section).toContainText("Upgrading to 2.62");
		await expect(page.getByTestId("maintenance-marker")).toContainText(
			"Under maintenance",
		);
	});

	test("declaring from the server page opens a window", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "about-to-move" });
		const server = await seedServer(sql, {
			name: "going-down",
			groupId: group.id,
		});
		await seedStatus(sql, { serverId: server.id, healthy: true });

		await page.goto(`/servers/${server.id}`);
		await page.getByRole("button", { name: "Declare maintenance" }).click();
		await page.getByLabel("What's being done").fill("Swapping the disk");
		await page.getByRole("button", { name: "Declare", exact: true }).click();

		await expect(
			page.getByTestId("maintenance-section"),
		).toContainText("Swapping the disk");
		expect(await openWindows(sql)).toBe(1);
	});

	test("lifting ends the window", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "back-already" });
		const server = await seedServer(sql, { name: "done", groupId: group.id });
		await seedStatus(sql, { serverId: server.id, healthy: true });
		await seedMaintenanceWindow(sql, { serverId: server.id, note: "Rebooting" });

		await page.goto(`/servers/${server.id}`);
		await page.getByRole("button", { name: "Lift" }).click();

		await expect(
			page.getByRole("button", { name: "Declare maintenance" }),
		).toBeVisible();
		expect(await openWindows(sql)).toBe(0);
	});

	test("a server under its group's window says so and points at the group", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "whole-region" });
		const server = await seedServer(sql, {
			name: "one-of-many",
			groupId: group.id,
		});
		await seedStatus(sql, { serverId: server.id, healthy: true });
		await seedMaintenanceWindow(sql, {
			serverGroupId: group.id,
			note: "Cutting over the database",
		});

		await page.goto(`/servers/${server.id}`);
		const covering = page.getByTestId("covering-group-window");
		await expect(covering).toContainText("Under maintenance, ending");
		await expect(covering).toContainText("Cutting over the database");
		await expect(
			covering.getByRole("link", { name: "whole-region" }),
		).toHaveAttribute("href", `/groups/${group.id}`);
		await expect(page.getByTestId("maintenance-marker")).toBeVisible();

		await page.goto(`/groups/${group.id}`);
		await expect(page.getByTestId("maintenance-marker")).toContainText(
			"Under maintenance",
		);
	});

	test("a just-ended window reads as settling, not as maintenance", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "coming-back" });
		const server = await seedServer(sql, {
			name: "just-returned",
			groupId: group.id,
		});
		await seedStatus(sql, { serverId: server.id, healthy: true });
		await seedMaintenanceWindow(sql, {
			serverId: server.id,
			endedMinutesAgo: 2,
			note: "Rebooted",
		});

		await page.goto(`/servers/${server.id}`);
		await expect(page.getByTestId("maintenance-marker")).toContainText(
			"Maintenance just ended",
		);
		await expect(
			page.getByRole("button", { name: "Declare maintenance" }),
		).toBeVisible();
	});

	test("the maintenance page lists what the fleet is not watching", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "whole-group" });
		await seedMaintenanceWindow(sql, {
			serverGroupId: group.id,
			note: "Cutting over the database",
		});

		await page.goto("/maintenance");
		const row = page.getByRole("row", { name: /whole-group/ });
		await expect(row).toContainText("Cutting over the database");
		await expect(row).toContainText("seed@bes.au");
		await expect(
			page.getByRole("link", { name: "whole-group" }),
		).toHaveAttribute("href", `/groups/${group.id}`);
	});

	test("nothing under maintenance says so", async ({ page }) => {
		await page.goto("/maintenance");
		await expect(
			page.getByText("Nothing is under maintenance"),
		).toBeVisible();
	});

	test("amending prefills the open window and keeps it the same window", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "running-long" });
		const server = await seedServer(sql, {
			name: "not-done-yet",
			groupId: group.id,
		});
		await seedStatus(sql, { serverId: server.id, healthy: true });
		await seedMaintenanceWindow(sql, {
			serverId: server.id,
			note: "Rebooting",
		});

		await page.goto(`/servers/${server.id}`);
		await page.getByRole("button", { name: "Amend" }).click();
		await expect(
			page.getByRole("heading", { name: /Amend maintenance/ }),
		).toBeVisible();
		const note = page.getByLabel("What's being done");
		await expect(note).toHaveValue("Rebooting");
		await note.fill("Rebooting, running long");
		await page.getByRole("button", { name: "Amend", exact: true }).click();

		await expect(
			page.getByTestId("maintenance-section"),
		).toContainText("Rebooting, running long");
		const rows = await sql.query<{ n: string; amended: string | null }>(
			"SELECT COUNT(*) AS n, MAX(amended_at::text) AS amended \
			 FROM maintenance_windows WHERE server_id = $1 AND ended_at IS NULL",
			[server.id],
		);
		expect(Number(rows[0]!.n)).toBe(1);
		expect(rows[0]!.amended).not.toBeNull();
	});

	test("an open incident offers the declaration over its target", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "mid-upgrade" });
		const server = await seedServer(sql, {
			name: "alerting-on-purpose",
			groupId: group.id,
		});
		await seedStatus(sql, { serverId: server.id, healthy: false });
		const issue = await seedIssue(sql, {
			serverId: server.id,
			source: "canopy",
			ref: "reachability",
			message: "unreachable",
		});
		const incident = await seedIncident(sql, {
			serverGroupId: group.id,
			issues: [{ issueId: issue.id }],
		});

		await page.goto(`/incidents/${incident.id}`);
		await page.getByRole("button", { name: "This is maintenance…" }).click();
		await page.getByLabel("What's being done").fill("It's us, upgrading");
		await page.getByRole("button", { name: "Declare", exact: true }).click();

		await expect(
			page.getByRole("heading", { name: /Declare maintenance/ }),
		).toBeHidden();
		await expect
			.poll(async () => {
				const rows = await sql.query<{ n: string }>(
					"SELECT COUNT(*) AS n FROM maintenance_windows \
					 WHERE server_group_id = $1 AND ended_at IS NULL",
					[group.id],
				);
				return Number(rows[0]!.n);
			})
			.toBe(1);
	});

	// spec: MNT#declaring, UPG
	test("declaring from an open plan carries the plan's window and note", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		const target = await seedVersion(sql, { major: 2, minor: 61, patch: 0 });
		// 22:00 to 02:00 is a four-hour window that wraps midnight, which is
		// what an upgrade slot usually looks like.
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: target.id,
			plannedFor: "2020-01-01",
			plannedTime: "22:00",
			plannedEndTime: "02:00",
			plannedZone: "Pacific/Fiji",
			note: "site can absorb 2.61 only",
		});

		await page.goto("/upgrades");
		await page
			.getByRole("button", { name: "Declare maintenance for kamaka" })
			.click();

		await expect(
			page.getByRole("heading", { name: "Declare maintenance — kamaka" }),
		).toBeVisible();
		await expect(page.getByLabel("What's being done")).toHaveValue(
			"site can absorb 2.61 only",
		);

		// The plan's slot is four hours long, so the declaration ends four
		// hours from now: a window says the work is happening now, and the
		// plan only supplies how long it takes.
		const endsAt = await page.getByLabel("Expected to end").inputValue();
		const hours = (new Date(endsAt).getTime() - Date.now()) / 3600_000;
		expect(hours).toBeGreaterThan(3.9);
		expect(hours).toBeLessThan(4.1);

		await page.getByRole("button", { name: "Declare", exact: true }).click();

		await expect
			.poll(async () => {
				const rows = await sql.query<{ note: string | null }>(
					"SELECT note FROM maintenance_windows \
					 WHERE server_group_id = $1 AND ended_at IS NULL",
					[group.id],
				);
				return rows.map((r) => r.note);
			})
			.toEqual(["site can absorb 2.61 only"]);
	});

	// spec: MNT#presentation
	test("a check the window skipped says so, and the legend names the mark", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "mid-cutover" });
		const server = await seedServer(sql, {
			name: "failing-on-purpose",
			groupId: group.id,
		});
		await seedStatus(sql, {
			serverId: server.id,
			healthy: false,
			health: [
				{ check: "database", healthy: false, message: "connection refused" },
			],
		});
		// What ingestion records under a window: the reported result stands,
		// the grade the window forces is what the fleet acts on.
		await sql.query(
			"UPDATE issues SET effective_result = 'skipped' WHERE server_id = $1",
			[server.id],
		);
		await seedMaintenanceWindow(sql, {
			serverId: server.id,
			note: "Cutting over the database",
		});

		await page.goto(`/servers/${server.id}`);
		await expect(
			page.getByTestId("check-maintenance-skip"),
		).toContainText("skipped: under maintenance");
		await expect(
			page.getByText("under maintenance (being worked on)"),
		).toBeVisible();
	});
});
