import type { Locator } from "@playwright/test";
import { expect, test } from "./test-fixtures";
import { type Sql, resetSeededTables, seedServer, seedStatus } from "./seed";

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
});
