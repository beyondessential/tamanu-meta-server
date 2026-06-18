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

	test("toggling 'Allow status from Tamanu' on persists to the server", async ({
		page,
		sql,
	}) => {
		// Saving requires a group, so seed one and put the server in it.
		const group = await seedServerGroup(sql, { name: "legacy-group" });
		const server = await seedServer(sql, {
			name: "legacy-target",
			groupId: group.id,
		});

		await page.goto(`/servers/${server.id}/edit`);

		// Off by default for a freshly-seeded server.
		const toggle = page.getByRole("checkbox", {
			name: "Allow status from Tamanu",
		});
		await expect(toggle).not.toBeChecked();

		await toggle.check();
		await page.getByRole("button", { name: /^save$/i }).click();

		// Save navigates to the detail page.
		await page.waitForURL(`**/servers/${server.id}`);

		const rows = await sql.query<{ allow_legacy_status: boolean }>(
			"SELECT allow_legacy_status FROM servers WHERE id = $1",
			[server.id],
		);
		expect(rows[0]!.allow_legacy_status).toBe(true);
	});
});

test.describe("archived view", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("lists archived servers and groups and restores them", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "arch-group" });
		const server = await seedServer(sql, { name: "arch-server", kind: "central" });
		// Archive both directly (the UI paths are covered elsewhere).
		await sql.query("UPDATE server_groups SET deleted_at = now() WHERE id = $1", [
			group.id,
		]);
		await sql.query("UPDATE servers SET deleted_at = now() WHERE id = $1", [
			server.id,
		]);

		await page.goto("/servers/archived");

		// Both archived items are discoverable here (and nowhere else).
		await expect(page.getByRole("link", { name: "arch-group" })).toBeVisible();
		await expect(page.getByText(/arch-server/)).toBeVisible();

		// Restore the group (its row renders first, so the first Restore is it).
		await page.getByRole("button", { name: "Restore" }).first().click();
		await expect(
			page.getByRole("link", { name: "arch-group" }),
		).not.toBeVisible();
		// The archived server is still listed.
		await expect(page.getByText(/arch-server/)).toBeVisible();
	});

	test("an empty group can be archived from its detail page", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "empty-grp" });
		page.on("dialog", (d) => d.accept());

		await page.goto(`/groups/${group.id}`);
		await page.getByRole("button", { name: "Archive" }).click();
		// Redirects to the servers list, and the group now shows under Archived.
		await expect(page).toHaveURL(/\/servers$/);
		await page.goto("/servers/archived");
		await expect(page.getByRole("link", { name: "empty-grp" })).toBeVisible();
	});

	test("a group whose servers are all gone archives, cascading to them", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "gone-grp" });
		// No statuses seeded → both servers are "gone".
		await seedServer(sql, { name: "gone-1", kind: "central", groupId: group.id });
		await seedServer(sql, { name: "gone-2", kind: "facility", groupId: group.id });
		page.on("dialog", (d) => d.accept());

		await page.goto(`/groups/${group.id}`);
		// The Archive button is offered because every member is gone.
		await page.getByRole("button", { name: "Archive" }).click();
		await expect(page).toHaveURL(/\/servers$/);

		// The group and both servers are now archived.
		await page.goto("/servers/archived");
		await expect(page.getByRole("link", { name: "gone-grp" })).toBeVisible();
		await expect(page.getByText(/gone-1/)).toBeVisible();
		await expect(page.getByText(/gone-2/)).toBeVisible();
	});
});

test.describe("server create → setup → archive flow", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("creates a server in a group, surfaces its enrollment ticket, then archives it", async ({
		page,
		sql,
	}) => {
		// The detail page's get_detail resolves the latest version, so it needs
		// at least one published version present.
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const group = await seedServerGroup(sql, { name: "flow-group" });

		// Create — the in-group route pre-selects the group, so we only set the
		// (required) name. Default kind is facility, so there's a single "Name"
		// field (no "Name in Tamanu Mobile app").
		await page.goto(`/groups/${group.id}/servers/new`);
		await page.getByLabel(/^Name(\s*\*)?$/i).fill("flow-server");
		await page.getByRole("button", { name: "Create server" }).click();

		// Lands on the new server's detail page.
		await expect(page).toHaveURL(/\/servers\/[0-9a-f-]{36}$/);
		await expect(
			page.getByRole("heading", { level: 1, name: /flow-server/ }),
		).toBeVisible();

		// Setup — an unregistered server auto-mints an enrollment ticket, showing
		// the bestool register command for the operator to run.
		await expect(page.getByText(/hasn't checked in yet/i)).toBeVisible();
		await expect(page.getByText(/bestool canopy register/)).toBeVisible();

		// Archive — confirm the dialog; since the server is in a group, it
		// redirects to that group's page (not the servers list).
		await page.getByRole("button", { name: "Archive", exact: true }).click();
		const dialog = page.getByRole("dialog");
		await expect(dialog).toBeVisible();
		await dialog.getByRole("button", { name: "Archive", exact: true }).click();
		await expect(page).toHaveURL(new RegExp(`/groups/${group.id}$`));
	});
});
