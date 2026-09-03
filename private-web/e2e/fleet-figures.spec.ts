import type { Locator } from "@playwright/test";
import { expect, test } from "./test-fixtures";
import {
	type Sql,
	resetSeededTables,
	seedApplicationReport,
	seedMachine,
	seedMachineReport,
	seedServer,
	seedStatus,
} from "./seed";

const pg = (version: string) =>
	`PostgreSQL ${version} on x86_64-pc-linux-gnu, compiled by gcc`;

/// Three servers with a deliberate spread: two share a PostgreSQL major (on
/// different exact versions), a Tamanu release, and a bestool version, one
/// differs on all three, and one reports nothing at all.
async function seedFleet(sql: Sql) {
	const alpha = await seedServer(sql, { name: "fleet-alpha" });
	const beta = await seedServer(sql, { name: "fleet-beta" });
	const gamma = await seedServer(sql, { name: "fleet-gamma" });
	const silent = await seedServer(sql, { name: "fleet-silent" });

	await seedStatus(sql, {
		serverId: alpha.id,
		version: "2.54.3",
		extra: { pgVersion: pg("16.3"), bestoolVersion: "2.10.5", uptimeSecs: 100 },
	});
	await seedStatus(sql, {
		serverId: beta.id,
		version: "2.54.1",
		extra: { pgVersion: pg("16.4"), bestoolVersion: "2.10.5", uptimeSecs: 200 },
	});
	await seedStatus(sql, {
		serverId: gamma.id,
		version: "2.46.0",
		extra: { pgVersion: pg("13.2"), bestoolVersion: "2.4.7", uptimeSecs: 300 },
	});
	return { alpha, beta, gamma, silent };
}

/// One box carrying two workloads, beside a box carrying one. The releases
/// the two workloads run are the caller's, since whether they agree is the
/// difference between a machine landing in one cell of a crossing and in two.
async function seedTwinBox(
	sql: Sql,
	releases: { central: string; facility: string },
) {
	const box = await seedMachine(sql, { name: "twin-box" });
	await seedMachineReport(sql, {
		machineId: box.id,
		extra: { osName: "Ubuntu", osVersion: "22.04" },
	});
	const central = await seedServer(sql, {
		name: "twin-central",
		type: "tamanu-central",
		machineId: box.id,
	});
	const facility = await seedServer(sql, {
		name: "twin-facility",
		type: "tamanu-facility",
		machineId: box.id,
	});
	await seedApplicationReport(sql, {
		applicationId: central.id,
		version: releases.central,
	});
	await seedApplicationReport(sql, {
		applicationId: facility.id,
		version: releases.facility,
	});

	const solo = await seedServer(sql, { name: "solo-box" });
	await seedMachineReport(sql, {
		machineId: solo.machineId,
		extra: { osName: "Ubuntu", osVersion: "22.04" },
	});
	await seedApplicationReport(sql, {
		applicationId: solo.id,
		version: "2.46.0",
	});

	return { box, central, facility, solo };
}

/// One cell of a crossing's table, addressed by both of its coordinates
/// rather than by position, since the axes reorder with the sort.
async function crossCell(crossTab: Locator, rowValue: string, colValue: string) {
	const headers = await crossTab.getByRole("columnheader").allTextContents();
	const column = headers.indexOf(colValue);
	expect(column, `no ${colValue} column`).toBeGreaterThan(0);
	const row = crossTab
		.getByRole("row")
		.filter({ has: crossTab.page().getByRole("rowheader", { name: rowValue }) });
	// The header row leads with a blank corner and a body row with its
	// rowheader, which is not a cell, so the data cells run one behind.
	return row.getByRole("cell").nth(column - 1);
}

/// Point a crossing's two axes at a pair of fields.
async function crossFields(crossTab: Locator, rowField: string, colField: string) {
	const page = crossTab.page();
	await crossTab.getByLabel("Rows").click();
	await page.getByRole("option", { name: rowField, exact: true }).click();
	await crossTab.getByLabel("Columns").click();
	await page.getByRole("option", { name: colField, exact: true }).click();
}

/// The rows of a spread, in the order they present: the value lines only,
/// not the card's sort control.
function valueRows(card: Locator) {
	return card.getByRole("button", { name: /: \d+$/ });
}

// spec: FIG#fleet-spread
test.describe("fleet figures", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("shows each figure's spread, and which servers hold a value", async ({
		page,
		sql,
	}) => {
		await seedFleet(sql);

		await page.goto("/servers/figures");

		// The card groups by PostgreSQL's major version, so the two servers on
		// 16.3 and 16.4 read as one population.
		const postgres = page.getByRole("group", { name: "PostgreSQL major" });
		await expect(postgres.getByRole("button", { name: "16: 2" })).toBeVisible();
		await expect(postgres.getByRole("button", { name: "13: 1" })).toBeVisible();
		// The server reporting nothing is counted, not hidden.
		await expect(
			postgres.getByRole("button", { name: "not reported: 1" }),
		).toBeVisible();

		// Same for Tamanu: the release branch, not the exact patch.
		const release = page.getByRole("group", { name: "Tamanu release" });
		await expect(release.getByRole("button", { name: "2.54: 2" })).toBeVisible();
		await expect(release.getByRole("button", { name: "2.46: 1" })).toBeVisible();

		// Expanding a value names the servers behind it.
		await postgres.getByRole("button", { name: "16: 2" }).click();
		await expect(page.getByRole("link", { name: "fleet-alpha" })).toBeVisible();
		await expect(page.getByRole("link", { name: "fleet-beta" })).toBeVisible();
	});

	test("looks up the exact version behind a coarse figure", async ({
		page,
		sql,
	}) => {
		await seedFleet(sql);

		await page.goto("/servers/figures");
		await page.getByRole("combobox", { name: "Field" }).fill("postgres");
		await page.getByRole("option", { name: "PostgreSQL version" }).click();

		const exact = page.getByRole("group", { name: "PostgreSQL version" });
		await expect(exact.getByRole("button", { name: "16.3: 1" })).toBeVisible();
		await expect(exact.getByRole("button", { name: "16.4: 1" })).toBeVisible();
		await expect(exact.getByRole("button", { name: "13.2: 1" })).toBeVisible();
	});

	test("reorders a spread by value, comparing versions as versions", async ({
		page,
		sql,
	}) => {
		await seedFleet(sql);
		// An older release, which orders below 2.46 as a version and above it
		// as text or as a decimal.
		const delta = await seedServer(sql, { name: "fleet-delta" });
		await seedStatus(sql, { serverId: delta.id, version: "2.9.4" });

		await page.goto("/servers/figures");

		const release = page.getByRole("group", { name: "Tamanu release" });
		await expect(valueRows(release).first()).toHaveAccessibleName("2.54: 2");

		await release.getByRole("button", { name: "Sort by value" }).click();
		const byValue = ["2.9: 1", "2.46: 1", "2.54: 2", "not reported: 1"];
		for (const [index, name] of byValue.entries()) {
			await expect(valueRows(release).nth(index)).toHaveAccessibleName(name);
		}

		// And back, on the same button.
		await release.getByRole("button", { name: "Sort by popularity" }).click();
		await expect(valueRows(release).first()).toHaveAccessibleName("2.54: 2");
	});

	test("looks up a field canopy derives no figure from", async ({
		page,
		sql,
	}) => {
		await seedFleet(sql);

		await page.goto("/servers/figures");
		await page.getByRole("combobox", { name: "Field" }).fill("uptimeSecs");

		// Three distinct values, one server each, plus the unreported server.
		await expect(page.getByRole("button", { name: "100: 1" })).toBeVisible();
		await expect(page.getByRole("button", { name: "300: 1" })).toBeVisible();
		await expect(
			page.getByRole("button", { name: "not reported: 1" }).first(),
		).toBeVisible();
	});

	test("looks up a field a healthcheck reports, as check.field", async ({
		page,
		sql,
	}) => {
		const { alpha, beta, gamma } = await seedFleet(sql);
		// Two servers share a disk-usage bucket, the third differs; the fourth
		// runs no such check at all.
		await seedStatus(sql, {
			serverId: alpha.id,
			health: [{ check: "diskspace", result: "warning", percent: 91 }],
		});
		await seedStatus(sql, {
			serverId: beta.id,
			health: [{ check: "diskspace", result: "warning", percent: 91 }],
		});
		await seedStatus(sql, {
			serverId: gamma.id,
			health: [{ check: "diskspace", result: "passed", percent: 12 }],
		});

		await page.goto("/servers/figures");
		await page
			.getByRole("combobox", { name: "Field" })
			.fill("diskspace.percent");

		await expect(page.getByRole("button", { name: "91: 2" })).toBeVisible();
		await expect(page.getByRole("button", { name: "12: 1" })).toBeVisible();

		// The check's own grade is a field like any other.
		await page.getByRole("combobox", { name: "Field" }).fill("diskspace.result");
		await expect(
			page.getByRole("button", { name: "warning: 2" }),
		).toBeVisible();
		await expect(page.getByRole("button", { name: "passed: 1" })).toBeVisible();
	});

	test("crosses two fields into a table of counts", async ({ page, sql }) => {
		await seedFleet(sql);

		await page.goto("/servers/figures");

		const crossTab = page.getByRole("group", { name: "Cross two fields" });
		await crossTab.getByLabel("Rows").click();
		await page.getByRole("option", { name: "PostgreSQL major", exact: true }).click();
		await crossTab.getByLabel("Columns").click();
		await page.getByRole("option", { name: "bestool", exact: true }).click();

		// 16 × 2.10.5 holds both alpha and beta; the diagonal is the only
		// populated part, since each PostgreSQL major pairs with one bestool.
		const table = crossTab.getByRole("table");
		await expect(table.getByRole("cell", { name: "2", exact: true })).toBeVisible();
		await table.getByRole("cell", { name: "2", exact: true }).click();
		await expect(page.getByRole("link", { name: "fleet-alpha" })).toBeVisible();
		await expect(page.getByRole("link", { name: "fleet-beta" })).toBeVisible();
	});

	test("reorders a crossing's rows and columns by value", async ({
		page,
		sql,
	}) => {
		await seedFleet(sql);

		await page.goto("/servers/figures");

		// The crossing leads with the coarse version figures.
		const crossTab = page.getByRole("group", { name: "Cross two fields" });
		await expect(crossTab.getByRole("rowheader").first()).toHaveText("16");
		await expect(
			crossTab.getByRole("columnheader").nth(1),
		).toHaveText("2.54");

		await crossTab.getByRole("button", { name: "Sort by value" }).click();
		await expect(crossTab.getByRole("rowheader").first()).toHaveText("13");
		await expect(
			crossTab.getByRole("columnheader").nth(1),
		).toHaveText("2.46");
	});

	/// A figure about the box spreads over boxes. Two workloads on one machine
	/// is one platform, not two, or every shared box would double-count the
	/// operating system it runs.
	// spec: FIG#machine-figures
	test("a machine figure spreads over machines, a shared box counting once", async ({
		page,
		sql,
	}) => {
		await seedTwinBox(sql, { central: "2.54.3", facility: "2.54.1" });

		await page.goto("/servers/figures");

		// Three applications, two boxes, one platform value.
		const platform = page.getByRole("group", { name: "Platform" });
		await expect(
			platform.getByRole("button", { name: "Ubuntu 22.04: 2" }),
		).toBeVisible();
		await expect(platform.getByText("2 machines")).toBeVisible();

		// And what it names are the boxes, not the workloads on them.
		await platform.getByRole("button", { name: "Ubuntu 22.04: 2" }).click();
		await expect(page.getByRole("link", { name: "twin-box" })).toBeVisible();
		await expect(page.getByRole("link", { name: "solo-box" })).toBeVisible();
		await expect(
			page.getByRole("link", { name: "twin-central" }),
		).toHaveCount(0);
	});

	/// A figure about the workload spreads over workloads, so the same shared
	/// box contributes each of its applications.
	// spec: FIG#fleet-spread
	test("an application figure spreads over applications", async ({
		page,
		sql,
	}) => {
		await seedTwinBox(sql, { central: "2.54.3", facility: "2.54.1" });

		await page.goto("/servers/figures");

		const release = page.getByRole("group", { name: "Tamanu release" });
		// Both workloads on the shared box are on 2.54, and both are counted.
		await expect(release.getByRole("button", { name: "2.54: 2" })).toBeVisible();
		await expect(release.getByRole("button", { name: "2.46: 1" })).toBeVisible();
		await expect(release.getByText("3 applications")).toBeVisible();

		await release.getByRole("button", { name: "2.54: 2" }).click();
		await expect(page.getByRole("link", { name: "twin-central" })).toBeVisible();
		await expect(
			page.getByRole("link", { name: "twin-facility" }),
		).toBeVisible();
	});

	/// A crossing counts boxes whichever grain its axes belong to, and says so,
	/// because a count that silently changes unit between the cards and the
	/// table is a count an operator cannot act on.
	// spec: FIG#crossings
	test("a crossing counts machines, and names the unit it counts", async ({
		page,
		sql,
	}) => {
		await seedTwinBox(sql, { central: "2.54.3", facility: "2.54.1" });

		await page.goto("/servers/figures");

		const crossTab = page.getByRole("group", { name: "Cross two fields" });
		await crossFields(crossTab, "Platform", "Tamanu release");

		await expect(crossTab.getByText("counting 2 machines")).toBeVisible();

		// The shared box's two workloads agree on their release, so the box is
		// one machine in one cell rather than two applications in it.
		const cell = await crossCell(crossTab, "Ubuntu 22.04", "2.54");
		await expect(cell).toHaveText("1");
		await cell.click();
		await expect(page.getByRole("link", { name: "twin-box" })).toBeVisible();
		await expect(
			page.getByRole("link", { name: "twin-central" }),
		).toHaveCount(0);
	});

	/// A box whose workloads disagree has no single value on an application
	/// axis, so it takes each of them. Its cells then sum to more than the
	/// fleet, which is the truthful reading: the box is on both releases.
	// spec: FIG#crossings
	test("a machine whose applications disagree appears in each matching cell", async ({
		page,
		sql,
	}) => {
		await seedTwinBox(sql, { central: "2.54.3", facility: "2.46.0" });

		await page.goto("/servers/figures");

		const crossTab = page.getByRole("group", { name: "Cross two fields" });
		await crossFields(crossTab, "Platform", "Tamanu release");

		// Two boxes counted, but the shared one is in both cells, so they sum
		// to three.
		await expect(crossTab.getByText("counting 2 machines")).toBeVisible();
		await expect(await crossCell(crossTab, "Ubuntu 22.04", "2.54")).toHaveText(
			"1",
		);
		const shared = await crossCell(crossTab, "Ubuntu 22.04", "2.46");
		await expect(shared).toHaveText("2");

		// The 2.46 cell holds both boxes: the shared one on its facility's
		// release, and the box that only runs 2.46.
		await shared.click();
		await expect(page.getByRole("link", { name: "twin-box" })).toBeVisible();
		await expect(page.getByRole("link", { name: "solo-box" })).toBeVisible();
	});

	/// A box that reports no operating system is not a box with no platform:
	/// the database engine its workload reports names its build toolchain, and
	/// that separates Windows from everything else.
	// spec: FIG#machine-figures
	test("a machine reporting no OS falls back to its application's database engine", async ({
		page,
		sql,
	}) => {
		const windows = await seedServer(sql, { name: "win-box" });
		await seedApplicationReport(sql, {
			applicationId: windows.id,
			extra: {
				pgVersion:
					"PostgreSQL 16.3, compiled by Visual C++ build 1940, 64-bit",
			},
		});
		const linux = await seedServer(sql, { name: "linux-box" });
		await seedApplicationReport(sql, {
			applicationId: linux.id,
			extra: { pgVersion: pg("16.3") },
		});

		await page.goto("/servers/figures");

		// Neither box reported an operating system, and neither reads as
		// unreported.
		const platform = page.getByRole("group", { name: "Platform" });
		await expect(
			platform.getByRole("button", { name: "Windows: 1" }),
		).toBeVisible();
		await expect(
			platform.getByRole("button", { name: "Linux: 1" }),
		).toBeVisible();

		await platform.getByRole("button", { name: "Windows: 1" }).click();
		await expect(page.getByRole("link", { name: "win-box" })).toBeVisible();
	});
});
