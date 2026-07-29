import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServer,
	seedServerGroup,
	seedServerGroupDomain,
} from "./seed";

// The e2e fixture runs the private-server in a debug build, so the Tailscale
// auth bypass treats every caller as `admin@localhost` (an admin). It also
// configures two managed zones: `tamanu.app` and `demo.tamanu.app`.

test.describe("group domains", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("empty state invites a claim and names the claimable zones", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "domainless" });
		await page.goto(`/groups/${group.id}`);

		await expect(
			page.getByRole("heading", { name: "Domains" }),
		).toBeVisible();
		await expect(
			page.getByText(/this group controls no domains yet/i),
		).toBeVisible();
		await expect(
			page.getByText(/must be at or under one of: tamanu\.app, demo\.tamanu\.app/i),
		).toBeVisible();
	});

	test("claimed domains list with the zone they resolve to", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
			createdBy: "operator@example.test",
		});
		// Sits under the nested zone, so it resolves to the longer apex.
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.demo.tamanu.app",
		});
		// No configured zone covers this one any more.
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "old.senaite.app",
		});

		await page.goto(`/groups/${group.id}`);

		await expect(page.getByText("fiji.tamanu.app", { exact: true })).toBeVisible();
		await expect(page.getByText("zone tamanu.app")).toBeVisible();
		await expect(page.getByText("zone demo.tamanu.app")).toBeVisible();
		await expect(page.getByText("no matching zone")).toBeVisible();
		await expect(page.getByText(/by operator@example\.test/)).toBeVisible();
	});

	test("an admin claims a domain and releases it again", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "samoa" });
		await page.goto(`/groups/${group.id}`);

		await page.getByLabel("Domain").fill("samoa.tamanu.app");
		await page.getByRole("button", { name: "Claim" }).click();

		await expect(
			page.getByText("samoa.tamanu.app", { exact: true }),
		).toBeVisible();
		await expect(page.getByText("zone tamanu.app")).toBeVisible();

		page.once("dialog", (dialog) => dialog.accept());
		await page.getByRole("button", { name: "Release samoa.tamanu.app" }).click();

		await expect(
			page.getByText(/this group controls no domains yet/i),
		).toBeVisible();
	});

	test("a domain outside every managed zone is refused", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "outside" });
		await page.goto(`/groups/${group.id}`);

		await page.getByLabel("Domain").fill("nope.example.com");
		await page.getByRole("button", { name: "Claim" }).click();

		await expect(
			page.getByText(/not within any of Canopy's managed DNS zones/i),
		).toBeVisible();
		await expect(
			page.getByText(/this group controls no domains yet/i),
		).toBeVisible();
	});

	test("a domain overlapping another group's claim is refused", async ({
		page,
		sql,
	}) => {
		const holder = await seedServerGroup(sql, { name: "holder" });
		await seedServerGroupDomain(sql, {
			groupId: holder.id,
			domain: "contested.tamanu.app",
		});
		const rival = await seedServerGroup(sql, { name: "rival" });

		await page.goto(`/groups/${rival.id}`);
		await page.getByLabel("Domain").fill("sub.contested.tamanu.app");
		await page.getByRole("button", { name: "Claim" }).click();

		await expect(page.getByText(/overlaps contested\.tamanu\.app/i)).toBeVisible();
	});
});

test.describe("server name-management permissions", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("a server shows no permission until one is granted", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "perm-group" });
		const server = await seedServer(sql, {
			name: "unpermitted",
			groupId: group.id,
		});
		await page.goto(`/servers/${server.id}`);

		await expect(page.getByText("Name management")).toBeVisible();
		await expect(page.getByText("Not permitted")).toBeVisible();
	});

	test("granting DNS in the edit form shows on the server", async ({
		page,
		sql,
	}) => {
		// The edit form requires a group, so grant on a grouped server.
		const group = await seedServerGroup(sql, { name: "grant-group" });
		const server = await seedServer(sql, {
			name: "grantee",
			groupId: group.id,
		});
		await page.goto(`/servers/${server.id}/edit`);

		await page.getByLabel("May manage its own DNS records").check();
		await page.getByRole("button", { name: "Save" }).click();

		await expect(page.getByText("DNS only")).toBeVisible();

		// And the other grant is independent of it.
		await page.goto(`/servers/${server.id}/edit`);
		await expect(
			page.getByLabel("May manage its own DNS records"),
		).toBeChecked();
		await expect(
			page.getByLabel("May obtain its own TLS certificates"),
		).not.toBeChecked();
		await page.getByLabel("May obtain its own TLS certificates").check();
		await page.getByRole("button", { name: "Save" }).click();

		await expect(page.getByText("DNS and TLS")).toBeVisible();
	});
});
