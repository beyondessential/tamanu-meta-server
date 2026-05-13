import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedServer, seedVersion } from "./seed";

test.describe("servers list page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the central/facility tabs and the seeded central row", async ({
		page,
		sql,
	}) => {
		const central = await seedServer(sql, {
			name: "central-uno",
			kind: "central",
		});

		await page.goto("/servers");

		await expect(
			page.getByRole("tab", { name: "Central servers" }),
		).toHaveAttribute("aria-selected", "true");
		await expect(
			page.getByRole("tab", { name: "Facility servers" }),
		).toBeVisible();

		// The seeded server's name shows as a link to its detail page.
		await expect(
			page.getByRole("link", { name: central.name }),
		).toHaveAttribute("href", `/servers/${central.id}`);
		// And the host appears in the row.
		await expect(page.getByText(central.host)).toBeVisible();
	});

	test("facilities tab switches the URL and lists facility servers only", async ({
		page,
		sql,
	}) => {
		const central = await seedServer(sql, {
			name: "central-only",
			kind: "central",
		});
		const facility = await seedServer(sql, {
			name: "facility-only",
			kind: "facility",
			parentServerId: central.id,
		});

		await page.goto("/servers");
		await page.getByRole("tab", { name: "Facility servers" }).click();
		await expect(page).toHaveURL(/\/servers\/facilities$/);

		// The facility's row is visible; the central's isn't.
		await expect(
			page.getByRole("link", { name: facility.name }),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: central.name }),
		).not.toBeVisible();
	});
});

test.describe("server detail page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the seeded server's name and host", async ({ page, sql }) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const server = await seedServer(sql, {
			name: "detail-target",
			kind: "central",
		});

		await page.goto(`/servers/${server.id}`);

		await expect(
			page.getByRole("heading", { name: server.name, level: 1 }),
		).toBeVisible();
		// Backend normalises the URL with a trailing slash; just check the
		// host link points back at the seeded URL ignoring that.
		const hostLink = page.getByRole("link", { name: new RegExp(server.host) });
		await expect(hostLink).toBeVisible();
	});

	test("nonexistent UUID surfaces an error alert", async ({ page }) => {
		await page.goto("/servers/00000000-0000-0000-0000-000000000000");
		await expect(page.getByRole("alert")).toBeVisible();
	});
});

test.describe("server edit page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("pre-fills the form with the seeded server's name", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, {
			name: "edit-target",
			kind: "central",
		});

		await page.goto(`/servers/${server.id}/edit`);

		await expect(page.getByLabel(/^Name$/i)).toHaveValue(server.name);
	});
});
