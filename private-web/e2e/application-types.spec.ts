import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServer,
	seedServerGroup,
	seedStatus,
	seedVersion,
} from "./seed";

/// Coverage for the type axis: how an application's type is presented, and how
/// its version is presented given what the type actually has.
///
/// spec: APP
test.describe("application types", () => {
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
			type: "tamanu-central",
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
			type: "senaite",
		});
		// A SENAITE agent reports health and figures but no application version.
		await seedStatus(sql, {
			serverId: server.id,
			version: null,
			extra: { pgVersion: "PostgreSQL 16.2", bestoolVersion: "2.10.5" },
		});

		await page.goto(`/servers/${server.id}`);

		// One chip carries what it is. There is no separate role chip: SENAITE
		// instances hold no role relative to each other, so the software alone
		// names the type.
		await expect(page.getByText("SENAITE", { exact: true })).toBeVisible();
		await expect(page.getByText("standalone", { exact: true })).toHaveCount(0);
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
			type: "tamanu-central",
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
			type: "tamanu-central",
			groupId: group.id,
		});
		await seedServer(sql, {
			name: "pacific-lims",
			type: "senaite",
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

	test("the machine form asks nothing about what runs on the box", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "target-group" });

		await page.goto(`/groups/${group.id}/machines/new`);

		// A type is reported, never entered, so none of what used to be asked
		// here is offered: no product, no role, no rank, no URL.
		await expect(page.getByRole("combobox", { name: "Product" })).toHaveCount(0);
		await expect(page.getByRole("combobox", { name: "Kind" })).toHaveCount(0);
		await expect(page.getByRole("combobox", { name: "Rank" })).toHaveCount(0);
		await expect(page.getByLabel("URL")).toHaveCount(0);
		await expect(page.getByLabel("Name in Tamanu Mobile app")).toHaveCount(0);

		// What the box is, where it is, and how it is watched: that is a
		// machine's form.
		await expect(page.getByRole("combobox", { name: "Location" })).toBeVisible();
		await expect(page.getByLabel("Tailscale identity")).toBeVisible();
		await expect(page.getByLabel("Monitor this machine")).toBeVisible();
	});

	test("the public-name field follows the application's type", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "listing-group" });
		const central = await seedServer(sql, {
			name: "listing-central",
			type: "tamanu-central",
			groupId: group.id,
		});
		const facility = await seedServer(sql, {
			name: "listing-facility",
			type: "tamanu-facility",
			groupId: group.id,
		});

		// A central is what the mobile app lists, so its edit form offers the
		// public name.
		await page.goto(`/servers/${central.id}/edit`);
		await expect(page.getByLabel("Name in Tamanu Mobile app")).toBeVisible();

		// A facility sits behind someone else's NAT and is nobody's to look up.
		await page.goto(`/servers/${facility.id}/edit`);
		await expect(page.getByLabel("Name in Tamanu Mobile app")).toHaveCount(0);
	});

	test("the fleet version spread covers only servers that have a version", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const central = await seedServer(sql, {
			name: "fleet-central",
			type: "tamanu-central",
		});
		const lims = await seedServer(sql, {
			name: "fleet-lims",
			type: "senaite",
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

	test("a canopy instance presents its own version ungraded", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const server = await seedServer(sql, {
			name: "canopy-self",
			type: "canopy",
		});
		// A canopy instance reports its own build version, which is nowhere near
		// Tamanu's release numbering.
		await seedStatus(sql, { serverId: server.id, version: "1.8.0" });

		await page.goto(`/servers/${server.id}`);

		// The version it reports is presented...
		await expect(page.getByText("1.8.0")).toBeVisible();
		// ...but graded against nothing: no distance from Tamanu's latest release,
		// and no link into a catalogue that doesn't describe it.
		await expect(
			page.getByRole("link", { name: /versions behind/ }),
		).toHaveCount(0);
		await expect(page.getByRole("link", { name: /1\.8\.0/ })).toHaveCount(0);
	});

	test("a crossing on the application version drops uncovered servers from both axes", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 34, patch: 1, status: "published" });
		const central = await seedServer(sql, {
			name: "cross-central",
			type: "tamanu-central",
		});
		const lims = await seedServer(sql, {
			name: "cross-lims",
			type: "senaite",
		});
		await seedStatus(sql, {
			serverId: central.id,
			version: "2.34.1",
			extra: { pgVersion: "PostgreSQL 16.2" },
		});
		// Distinct database major, so its presence or absence in the crossing is
		// unambiguous.
		await seedStatus(sql, {
			serverId: lims.id,
			version: null,
			extra: { pgVersion: "PostgreSQL 13.12" },
		});

		await page.goto("/servers/figures");

		// The crossing opens on the coarse version figures: database major
		// against release.
		const crossTab = page.getByRole("group", { name: "Cross two fields" });
		await expect(crossTab.getByRole("rowheader").first()).toHaveText("16");
		// The SENAITE server is dropped from the table entirely rather than
		// occupying an unreported column, so its database major never appears as
		// a row of its own.
		await expect(crossTab.getByRole("rowheader", { name: "13" })).toHaveCount(0);
		await expect(
			crossTab.getByRole("columnheader", { name: "not reported" }),
		).toHaveCount(0);
	});
});
