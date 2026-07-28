import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServer,
	seedServerGroup,
	seedStatus,
	seedVersion,
} from "./seed";

/// Coverage for the product axis: how a product is presented, and how its
/// version is presented given what the product actually has.
///
/// spec: APP
test.describe("server products", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("a Tamanu server presents its version graded against the catalogue", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const server = await seedServer(sql, {
			name: "central-graded",
			product: "tamanu",
			kind: "central",
		});
		await seedStatus(sql, { serverId: server.id, version: "2.34.1" });

		await page.goto(`/servers/${server.id}`);

		// Graded: the version links into the catalogue and carries its distance
		// from the latest release.
		await expect(
			page.getByRole("link", { name: /2\.34\.1 0 versions behind latest/ }),
		).toBeVisible();
	});

	test("a SENAITE server presents no version at all", async ({ page, sql }) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const server = await seedServer(sql, {
			name: "lims-box",
			product: "senaite",
		});
		// A SENAITE agent reports health and figures but no application version.
		await seedStatus(sql, {
			serverId: server.id,
			version: null,
			extra: { pgVersion: "PostgreSQL 16.2", bestoolVersion: "2.10.5" },
		});

		await page.goto(`/servers/${server.id}`);

		// The product chip identifies it, and its role is the standalone one
		// SENAITE defines.
		await expect(page.getByText("SENAITE", { exact: true })).toBeVisible();
		await expect(page.getByText("standalone", { exact: true })).toBeVisible();
		// There is no version affordance at all — not even "unknown". There is no
		// version to learn, so an unknown would read as a reporting failure.
		await expect(page.getByText("unknown")).toHaveCount(0);
		await expect(page.getByRole("link", { name: /versions behind/ })).toHaveCount(
			0,
		);
	});

	test("a Tamanu server that has not reported a version presents unknown", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const server = await seedServer(sql, {
			name: "silent-central",
			product: "tamanu",
			kind: "central",
		});
		// Reported, but carrying no version — the agent couldn't read it.
		await seedStatus(sql, { serverId: server.id, version: null });

		await page.goto(`/servers/${server.id}`);

		// There *is* a version to learn here, so the indicator says so.
		await expect(page.getByText("unknown")).toBeVisible();
	});

	test("a mixed-product group takes its headline version from the Tamanu member", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const group = await seedServerGroup(sql, { name: "pacific-mixed" });
		const central = await seedServer(sql, {
			name: "pacific-central",
			product: "tamanu",
			kind: "central",
			groupId: group.id,
		});
		await seedServer(sql, {
			name: "pacific-lims",
			product: "senaite",
			groupId: group.id,
			rank: "production",
		});
		await seedStatus(sql, { serverId: central.id, version: "2.34.1" });

		await page.goto("/status");

		// The group card shows the Tamanu member's version rather than blanking
		// because one of its members has none to give.
		await expect(page.getByText(group.name)).toBeVisible();
		await expect(page.getByText("2.34.1")).toBeVisible();
	});

	test("the create form offers a product and narrows the roles to match", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "target-group" });

		await page.goto(`/groups/${group.id}/servers/new`);

		// Tamanu is the default product, and offers both of its roles.
		const kindField = page.getByRole("combobox", { name: "Kind" });
		await expect(kindField).toHaveText("facility");
		await kindField.click();
		await expect(page.getByRole("option", { name: "central" })).toBeVisible();
		await expect(page.getByRole("option", { name: "facility" })).toBeVisible();
		await expect(page.getByRole("option", { name: "standalone" })).toHaveCount(
			0,
		);
		await page.keyboard.press("Escape");

		// Choosing SENAITE moves the role to the one it defines, since it has no
		// central.
		await page.getByRole("combobox", { name: "Product" }).click();
		await page.getByRole("option", { name: "SENAITE" }).click();
		await expect(kindField).toHaveText("standalone");
	});

	test("the public-name field is only offered for a publicly-listable product", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "listing-group" });

		await page.goto(`/groups/${group.id}/servers/new`);

		// The form opens on facility, which is not listed either; a central is.
		await expect(page.getByLabel("Name in Tamanu Mobile app")).toHaveCount(0);
		await page.getByRole("combobox", { name: "Kind" }).click();
		await page.getByRole("option", { name: "central" }).click();
		await expect(page.getByLabel("Name in Tamanu Mobile app")).toBeVisible();

		// SENAITE cannot be listed at all, so choosing it takes the field away
		// even though the role moves to standalone rather than staying central.
		await page.getByRole("combobox", { name: "Product" }).click();
		await page.getByRole("option", { name: "SENAITE" }).click();
		await expect(page.getByLabel("Name in Tamanu Mobile app")).toHaveCount(0);
	});

	test("the fleet version spread covers only servers that have a version", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const central = await seedServer(sql, {
			name: "fleet-central",
			product: "tamanu",
			kind: "central",
		});
		const lims = await seedServer(sql, {
			name: "fleet-lims",
			product: "senaite",
		});
		await seedStatus(sql, { serverId: central.id, version: "2.34.1" });
		await seedStatus(sql, {
			serverId: lims.id,
			version: null,
			extra: { pgVersion: "PostgreSQL 16.2" },
		});

		await page.goto("/servers/figures");

		// The release spread counts the one server that has a release, and the
		// SENAITE server is absent from it rather than counted as having failed
		// to report one.
		const release = page.getByRole("group", { name: "Tamanu release" });
		await expect(release.getByRole("button", { name: "2.34: 1" })).toBeVisible();
		await expect(
			release.getByRole("button", { name: /not reported/ }),
		).toHaveCount(0);

		// The database-engine spread still covers the whole fleet: that figure
		// has nothing to do with which product a server runs.
		const postgres = page.getByRole("group", { name: "PostgreSQL major" });
		await expect(postgres.getByRole("button", { name: "16: 1" })).toBeVisible();
	});
});
