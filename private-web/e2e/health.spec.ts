import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServer,
	seedStatus,
	seedVersion,
} from "./seed";

test.describe("server detail health indicator", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("shows the Unhealthy chip when the latest status is unhealthy", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const server = await seedServer(sql, {
			name: "sick-server",
			kind: "central",
		});
		await seedStatus(sql, {
			serverId: server.id,
			healthy: false,
			health: [{ check: "database", healthy: false, message: "connection refused" }],
		});

		await page.goto(`/servers/${server.id}`);

		// Healthy chip lives in the InfoSection header.
		await expect(page.getByText("Unhealthy", { exact: true })).toBeVisible();
		await expect(
			page.getByText("Healthy", { exact: true }),
		).not.toBeVisible();
	});

	test("shows the Healthy chip when the latest status is healthy", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const server = await seedServer(sql, {
			name: "ok-server",
			kind: "central",
		});
		await seedStatus(sql, {
			serverId: server.id,
			healthy: true,
			health: [],
		});

		await page.goto(`/servers/${server.id}`);

		await expect(page.getByText("Healthy", { exact: true })).toBeVisible();
	});
});

test.describe("server detail checks table", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders each check with its extras and orders failing first", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const server = await seedServer(sql, {
			name: "checked-server",
			kind: "central",
		});
		await seedStatus(sql, {
			serverId: server.id,
			healthy: true,
			health: [
				{ check: "always-passes", healthy: true },
				{
					check: "almost-broken",
					healthy: false,
					hint: "free_pct: 2",
				},
			],
		});

		await page.goto(`/servers/${server.id}`);

		// Both check names are listed somewhere in the page.
		await expect(page.getByText("almost-broken")).toBeVisible();
		await expect(page.getByText("always-passes")).toBeVisible();

		// The failing entry surfaces its extras as a key/value line.
		await expect(page.getByText("hint")).toBeVisible();
		await expect(page.getByText("free_pct: 2")).toBeVisible();
	});
});
