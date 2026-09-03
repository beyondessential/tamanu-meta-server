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

	test("the detail page names the grants only where one is held", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "perm-group" });
		const plain = await seedServer(sql, {
			name: "unpermitted",
			groupId: group.id,
		});
		const granted = await seedServer(sql, {
			name: "granted",
			groupId: group.id,
			mayManageDns: true,
			mayManageTls: true,
		});

		// Nothing granted: the row says nothing worth a line. "Not permitted" on
		// every server in the fleet advertises a feature a deployment without DNS
		// zones does not have.
		await page.goto(`/fleet/applications/${plain.id}`);
		await expect(
			page.getByRole("heading", { level: 1, name: /unpermitted/ }),
		).toBeVisible();
		await expect(page.getByText("Name management")).toHaveCount(0);

		await page.goto(`/fleet/applications/${granted.id}`);
		await expect(page.getByText("Name management")).toBeVisible();
		await expect(page.getByText("DNS and TLS")).toBeVisible();
	});

	test("a group with no domain disables the grants and says why", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "domainless" });
		const server = await seedServer(sql, { name: "central", groupId: group.id });

		await page.goto(`/fleet/machines/${server.machineId}/edit`);
		await expect(page.getByTestId("application-section")).toBeVisible();

		// A grant is exercised over names beneath a domain the group controls, so
		// without one there is nothing for it to authorise.
		await expect(
			page.getByLabel("May manage its own DNS records"),
		).toBeDisabled();
		await expect(
			page.getByLabel("May obtain its own TLS certificates"),
		).toBeDisabled();
		await expect(page.getByText(/group controls no domain/i)).toBeVisible();
	});

	test("granting DNS in the edit form shows on the server", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "grant-group" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "grant.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "grantee",
			groupId: group.id,
		});
		await page.goto(`/fleet/machines/${server.machineId}/edit`);

		// The domain the group controls is named, so an operator can see what the
		// grant would cover.
		await expect(
			page.getByText(/only to names under grant\.tamanu\.app/i),
		).toBeVisible();

		await page.getByLabel("May manage its own DNS records").check();
		await page.getByRole("button", { name: "Save" }).click();

		// Saving lands on the box, whose form it is; the grant reads back on the
		// application it belongs to. Wait for the landing first — navigating
		// away while the form's writes are in flight races them.
		await page.waitForURL(`**/fleet/machines/${server.machineId}`);
		await page.goto(`/fleet/applications/${server.id}`);
		await expect(page.getByText("DNS only")).toBeVisible();

		// And the other grant is independent of it.
		await page.goto(`/fleet/machines/${server.machineId}/edit`);
		await expect(
			page.getByLabel("May manage its own DNS records"),
		).toBeChecked();
		await expect(
			page.getByLabel("May obtain its own TLS certificates"),
		).not.toBeChecked();
		await page.getByLabel("May obtain its own TLS certificates").check();
		await page.getByRole("button", { name: "Save" }).click();

		await page.waitForURL(`**/fleet/machines/${server.machineId}`);
		await page.goto(`/fleet/applications/${server.id}`);
		await expect(page.getByText("DNS and TLS")).toBeVisible();
	});

	// The endpoint's own reading of "unconfigured" is covered in Rust
	// (`domains::grant_availability_reports_unconfigured_*`); the e2e fixture
	// always has zones configured, so the response is stubbed here to exercise the
	// branch of the form that reads it.
	test("the grants are hidden entirely where the feature is not in use", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "unused" });
		const server = await seedServer(sql, { name: "central", groupId: group.id });

		await page.route("**/api/domains/grant_availability", (route) =>
			route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ state: "unconfigured", group_domains: [] }),
			}),
		);

		await page.goto(`/fleet/machines/${server.machineId}/edit`);
		await expect(page.getByTestId("application-section")).toBeVisible();
		await expect(page.getByText("Name management")).toHaveCount(0);
		await expect(
			page.getByLabel("May manage its own DNS records"),
		).toHaveCount(0);
	});

	test("a grant already held stays visible even where the feature is not in use", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "unused" });
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageDns: true,
		});

		await page.route("**/api/domains/grant_availability", (route) =>
			route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ state: "unconfigured", group_domains: [] }),
			}),
		);

		await page.goto(`/fleet/machines/${server.machineId}/edit`);
		// Hiding it would strand a grant with no way to withdraw it.
		const dns = page.getByLabel("May manage its own DNS records");
		await expect(dns).toBeChecked();
		await expect(dns).toBeEnabled();
		await expect(page.getByText(/still holds a grant/i)).toBeVisible();
	});
});
