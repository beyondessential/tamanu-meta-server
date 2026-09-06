import {
	resetSeededTables,
	seedApplicationReport,
	seedServer,
	seedServerGroup,
	seedVersion,
} from "./seed";
import { expect, test } from "./test-fixtures";

/// How a group's reporting-schema pairs are presented, and how an operator asks
/// for one to be built.
///
/// spec: RPT
test.describe("reporting schemas", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	/// One pair per version the group's Tamanu applications report running. A
	/// facility mid-rollout is on a different version from its central, so both
	/// are pairs: a schema follows the version rather than the application.
	///
	/// spec: RPT#pairs
	test("a group shows a pair per version its applications run", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 2, minor: 59, patch: 0, status: "published" });
		await seedVersion(sql, { major: 2, minor: 60, patch: 0, status: "published" });
		const group = await seedServerGroup(sql, { name: "kamaka" });

		const central = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			type: "tamanu-central",
		});
		const facility = await seedServer(sql, {
			name: "facility",
			groupId: group.id,
			type: "tamanu-facility",
		});
		await seedApplicationReport(sql, {
			applicationId: central.id,
			version: "2.60.0",
		});
		await seedApplicationReport(sql, {
			applicationId: facility.id,
			version: "2.59.0",
		});

		await page.goto(`/groups/${group.id}`);

		const section = page.getByTestId("reporting-schemas");
		await expect(section).toBeVisible();
		await expect(section.getByTestId("reporting-schema-row")).toHaveCount(2);
		await expect(section.getByText("2.59.0")).toBeVisible();
		await expect(section.getByText("2.60.0")).toBeVisible();

		// Nothing has been built yet, so both are awaiting one.
		await expect(section.getByText("Awaiting build")).toHaveCount(2);
	});

	/// An operator asking for a build is what reinstates a pair, so the ask has
	/// to be visible once made.
	///
	/// spec: RPT#pairs
	test("asking for a build records the ask", async ({ page, sql }) => {
		await seedVersion(sql, { major: 2, minor: 60, patch: 0, status: "published" });
		const group = await seedServerGroup(sql, { name: "kamaka" });
		const central = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			type: "tamanu-central",
		});
		await seedApplicationReport(sql, {
			applicationId: central.id,
			version: "2.60.0",
		});

		await page.goto(`/groups/${group.id}`);

		const section = page.getByTestId("reporting-schemas");
		await section.getByRole("button", { name: "Build sooner" }).click();

		await expect(section.getByText("Build asked for")).toBeVisible();

		const rows = await sql.query("SELECT requested_by FROM reporting_schema_requests");
		expect(rows.rows).toHaveLength(1);
	});
});
