import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedMaintenanceWindow,
	seedServer,
	seedServerGroup,
	seedStatus,
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
		await expect(section).toContainText("Under maintenance until");
		await expect(section).toContainText("Upgrading to 2.62");
		await expect(section).toContainText("daniel@bes.au");
		await expect(page.getByTestId("maintenance-marker")).toBeVisible();
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
		await expect(covering).toContainText("Under maintenance until");
		await expect(covering).toContainText("Cutting over the database");
		await expect(
			covering.getByRole("link", { name: "whole-region" }),
		).toHaveAttribute("href", `/groups/${group.id}`);
		await expect(page.getByTestId("maintenance-marker")).toBeVisible();
	});

	test("the maintenance page lists what the fleet is not watching", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "whole-deployment" });
		await seedMaintenanceWindow(sql, {
			serverGroupId: group.id,
			note: "Cutting over the database",
		});

		await page.goto("/maintenance");
		const row = page.getByRole("row", { name: /whole-deployment/ });
		await expect(row).toContainText("Cutting over the database");
		await expect(
			page.getByRole("link", { name: "whole-deployment" }),
		).toHaveAttribute("href", `/groups/${group.id}`);
	});

	test("nothing under maintenance says so", async ({ page }) => {
		await page.goto("/maintenance");
		await expect(
			page.getByText("Nothing is under maintenance"),
		).toBeVisible();
	});
});
