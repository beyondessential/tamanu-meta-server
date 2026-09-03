import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedApplicationReport,
	seedDevice,
	seedServer,
	seedServerGroup,
	seedVersion,
} from "./seed";

test.describe("servers list page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the fleet tabs and the seeded group row", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "cluster-uno" });
		await seedServer(sql, {
			name: "in-group",
			type: "tamanu-central",
			groupId: group.id,
		});

		await page.goto("/servers");

		await expect(
			page.getByRole("tab", { name: "Groups" }),
		).toHaveAttribute("aria-selected", "true");
		await expect(page.getByRole("tab", { name: "Archived" })).toBeVisible();
		await expect(page.getByRole("tab", { name: "Figures" })).toBeVisible();

		// The fleet is browsed by group, and every machine is in one, so there
		// is no ungrouped listing to offer.
		// spec: FLT
		await expect(page.getByRole("tab", { name: "Ungrouped" })).toHaveCount(0);
		await expect(page.getByRole("tab")).toHaveCount(3);

		// The group's name shows as a link to its detail page.
		await expect(
			page.getByRole("link", { name: group.name }),
		).toHaveAttribute("href", `/groups/${group.id}`);
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
			type: "tamanu-central",
			groupId: group.id,
		});

		await page.goto(`/applications/${server.id}`);

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

		// The server's Group section shows the group's effective billing labels.
		await expect(page.getByText("Billing labels")).toBeVisible();
		await expect(page.getByText("billing.product=tamanu")).toBeVisible();
		await expect(page.getByText("billing.deployment=host-group")).toBeVisible();
	});

	test("nonexistent UUID surfaces an error alert", async ({ page }) => {
		await page.goto("/applications/00000000-0000-0000-0000-000000000000");
		await expect(page.getByRole("alert")).toBeVisible();
	});

	/// The page moved off `/servers` because that word named the box and the
	/// workload at once. A link into Canopy outlives the rename — a bookmark, a
	/// Slack message, an incident writeup — so the old address still lands, and
	/// lands on the page rather than on a router that shrugs.
	///
	/// spec: FLT#navigating-the-two-grains
	test("the old /servers address lands on the application", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const server = await seedServer(sql, {
			name: "moved-target",
			type: "tamanu-central",
		});

		await page.goto(`/servers/${server.id}`);

		await expect(page).toHaveURL(new RegExp(`/applications/${server.id}$`));
		await expect(
			page.getByRole("heading", {
				level: 1,
				name: new RegExp(server.name),
			}),
		).toBeVisible();
	});

	test("the old /servers edit address lands on the edit form", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "moved-edit-target" });

		await page.goto(`/servers/${server.id}/edit`);

		await expect(page).toHaveURL(
			new RegExp(`/applications/${server.id}/edit$`),
		);
	});

	/// The fleet index is not an application, and shares only a word with the
	/// pages that moved. A greedy redirect would swallow its tabs.
	test("the fleet index tabs are not redirected", async ({ page }) => {
		await page.goto("/servers/figures");
		await expect(page).toHaveURL(/\/servers\/figures$/);

		await page.goto("/servers/archived");
		await expect(page).toHaveURL(/\/servers\/archived$/);
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
			type: "tamanu-central",
		});

		await page.goto(`/applications/${server.id}/edit`);

		// The label carries a required-field asterisk ("Name *"), and the central
		// edit form also has a "Name in Tamanu Mobile app" field — so match "Name"
		// with an optional trailing asterisk, anchored to exclude the latter.
		await expect(page.getByLabel(/^Name(\s*\*)?$/i)).toHaveValue(server.name);
	});

	test("saving a renamed server persists the change", async ({
		page,
		sql,
	}) => {
		// Saving requires a group, so seed one and put the server in it.
		const group = await seedServerGroup(sql, { name: "edit-group" });
		const server = await seedServer(sql, {
			name: "edit-save-target",
			groupId: group.id,
		});

		await page.goto(`/applications/${server.id}/edit`);

		const nameField = page.getByLabel(/^Name(\s*\*)?$/i);
		await nameField.fill("edit-save-renamed");
		await page.getByRole("button", { name: /^save$/i }).click();

		// Save navigates to the detail page.
		await page.waitForURL(`**/applications/${server.id}`);

		const rows = await sql.query<{ name: string }>(
			"SELECT name FROM applications WHERE id = $1",
			[server.id],
		);
		expect(rows[0]!.name).toBe("edit-save-renamed");
	});

	// The identity belongs to the box, so it is offered on the machine's form
	// and nowhere else. An application editing it would be editing the box's.
	test("offers no identity field", async ({ page, sql }) => {
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "edit-no-identity",
			deviceId: device.id,
		});

		await page.goto(`/applications/${server.id}/edit`);
		await expect(page.getByLabel(/^Name(\s*\*)?$/i)).toHaveValue(server.name);
		await expect(page.getByLabel(/device/i)).toHaveCount(0);
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
		const server = await seedServer(sql, { name: "arch-server", type: "tamanu-central" });
		// Archive both directly (the UI paths are covered elsewhere).
		await sql.query("UPDATE server_groups SET deleted_at = now() WHERE id = $1", [
			group.id,
		]);
		await sql.query("UPDATE applications SET deleted_at = now() WHERE id = $1", [
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
		await seedServer(sql, { name: "gone-1", type: "tamanu-central", groupId: group.id });
		await seedServer(sql, { name: "gone-2", type: "tamanu-facility", groupId: group.id });
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

	test("a group whose servers reported long ago still archives", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "quiet-grp" });
		const one = await seedServer(sql, {
			name: "quiet-1",
			type: "tamanu-central",
			groupId: group.id,
		});
		const two = await seedServer(sql, {
			name: "quiet-2",
			type: "tamanu-facility",
			groupId: group.id,
		});
		// Both reported months ago and nothing since, so both are unreachable
		// rather than never heard from. Archiving is still the right call: the
		// button asks whether every member has gone quiet, not what colour its
		// dot is.
		for (const s of [one, two]) {
			await seedApplicationReport(sql, {
				applicationId: s.id,
				reportedAt: "NOW() - INTERVAL '90 days'",
			});
		}
		page.on("dialog", (d) => d.accept());

		await page.goto(`/groups/${group.id}`);
		await page.getByRole("button", { name: "Archive" }).click();
		await expect(page).toHaveURL(/\/servers$/);

		await page.goto("/servers/archived");
		await expect(page.getByRole("link", { name: "quiet-grp" })).toBeVisible();
		await expect(page.getByText(/quiet-1/)).toBeVisible();
		await expect(page.getByText(/quiet-2/)).toBeVisible();
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

		// Create the box — the in-group route pre-selects the group, so we only
		// set the (required) name. Nothing here names an application: what runs
		// on the box arrives by report.
		await page.goto(`/groups/${group.id}/machines/new`);
		await page.getByLabel(/^Name(\s*\*)?$/i).fill("flow-machine");
		await page.getByRole("button", { name: "Create machine" }).click();

		// Lands on the new box's own page.
		await expect(page).toHaveURL(/\/machines\/[0-9a-f-]{36}$/);

		// Setup — enrolment admits the box, so an unenrolled machine auto-mints
		// a ticket and shows the bestool register command for the operator to
		// run. No application is involved: nothing runs here until the enrolled
		// agent reports it.
		await expect(page.getByText(/hasn't checked in yet/i)).toBeVisible();
		await expect(page.getByText(/bestool canopy register/)).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "Set up this machine" }),
		).toBeVisible();

		// The workload on it is reported, not entered, so seed one and carry on
		// through its lifecycle.
		const server = await seedServer(sql, {
			name: "flow-server",
			groupId: group.id,
		});
		await page.goto(`/applications/${server.id}`);
		await expect(
			page.getByRole("heading", { level: 1, name: /flow-server/ }),
		).toBeVisible();

		// An application never mints a ticket. It points at its box instead.
		await expect(page.getByText(/bestool canopy register/)).toHaveCount(0);
		await page.getByRole("link", { name: "Enrol its machine" }).click();
		await expect(page).toHaveURL(/\/machines\/[0-9a-f-]{36}$/);
		await page.goBack();

		// Archive — confirm the dialog; since the server is in a group, it
		// redirects to that group's page (not the servers list).
		await page.getByRole("button", { name: "Archive", exact: true }).click();
		const dialog = page.getByRole("dialog");
		await expect(dialog).toBeVisible();
		await dialog.getByRole("button", { name: "Archive", exact: true }).click();
		await expect(page).toHaveURL(new RegExp(`/groups/${group.id}$`));
	});
});
