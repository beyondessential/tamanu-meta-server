import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedServer, seedServerGroup, seedVersion } from "./seed";

test.describe("servers list page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the groups/ungrouped tabs and the seeded group row", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "cluster-uno" });
		await seedServer(sql, {
			name: "in-group",
			kind: "central",
			groupId: group.id,
		});

		await page.goto("/servers");

		await expect(
			page.getByRole("tab", { name: "Groups" }),
		).toHaveAttribute("aria-selected", "true");
		await expect(
			page.getByRole("tab", { name: "Ungrouped" }),
		).toBeVisible();

		// The group's name shows as a link to its detail page.
		await expect(
			page.getByRole("link", { name: group.name }),
		).toHaveAttribute("href", `/groups/${group.id}`);
	});

	test("ungrouped tab switches the URL and lists servers without a group", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "the-group" });
		const grouped = await seedServer(sql, {
			name: "is-grouped",
			groupId: group.id,
		});
		const orphan = await seedServer(sql, {
			name: "no-group",
			groupId: null,
		});

		await page.goto("/servers");
		await page.getByRole("tab", { name: "Ungrouped" }).click();
		await expect(page).toHaveURL(/\/servers\/ungrouped$/);

		await expect(
			page.getByRole("link", { name: new RegExp(orphan.name) }),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: new RegExp(grouped.name) }),
		).not.toBeVisible();
	});
});

test.describe("server detail page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the seeded server's name and host", async ({ page, sql }) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const group = await seedServerGroup(sql, { name: "host-group" });
		const server = await seedServer(sql, {
			name: "detail-target",
			kind: "central",
			groupId: group.id,
		});

		await page.goto(`/servers/${server.id}`);

		// The page renders two h1s (app bar "Canopy" + page heading);
		// scope to the heading whose accessible name includes the server
		// name, which the group prefix becomes part of.
		await expect(
			page.getByRole("heading", {
				level: 1,
				name: new RegExp(server.name),
			}),
		).toBeVisible();
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

		// The label carries a required-field asterisk ("Name *"), and the central
		// edit form also has a "Name in Tamanu Mobile app" field — so match "Name"
		// with an optional trailing asterisk, anchored to exclude the latter.
		await expect(page.getByLabel(/^Name(\s*\*)?$/i)).toHaveValue(server.name);
	});
});
