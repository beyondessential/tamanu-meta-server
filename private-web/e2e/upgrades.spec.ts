import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServerGroup,
	seedUpgradePlan,
	seedVersion,
} from "./seed";

test.describe("upgrades dashboard", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("separates planned deployments from ones with nothing recorded", async ({
		page,
		sql,
	}) => {
		const planned = await seedServerGroup(sql, { name: "kamaka" });
		const unplanned = await seedServerGroup(sql, { name: "drifting" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = ANY($1)",
			[[planned.id, unplanned.id]],
		);
		const target = await seedVersion(sql, { major: 2, minor: 61, patch: 0 });

		// Long past, so `late` holds however far in the future this runs.
		await seedUpgradePlan(sql, {
			groupId: planned.id,
			targetVersionId: target.id,
			plannedFor: "2020-01-01",
			note: "site can absorb 2.61 only",
		});

		await page.goto("/upgrades");

		const plannedRow = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		await expect(plannedRow).toContainText("2.60.0");
		await expect(plannedRow).toContainText("2.61.0");
		await expect(plannedRow).toContainText("site can absorb 2.61 only");
		await expect(plannedRow).toContainText("late");
		// The plan says where it is going; the verdict says whether the data
		// survives getting there. Untested until a consumer reports.
		await expect(plannedRow).toContainText("not yet tested");

		// The deployment with nothing recorded is the one this view exists to
		// surface, so it is listed rather than omitted.
		await expect(
			page.getByTestId("unplanned-upgrade-row").filter({ hasText: "drifting" }),
		).toBeVisible();
		await expect(page.getByTestId("unplanned-upgrades")).not.toContainText(
			"kamaka",
		);
	});

	test("says so when nothing is planned", async ({ page, sql }) => {
		await seedServerGroup(sql, { name: "quiet" });

		await page.goto("/upgrades");

		await expect(page.getByTestId("planned-upgrades")).toContainText(
			"No deployment has a recorded plan",
		);
	});

	test("withdrawing a plan puts the deployment back to unplanned", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = $1",
			[group.id],
		);
		const target = await seedVersion(sql, { major: 2, minor: 61, patch: 0 });
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: target.id,
		});

		await page.goto("/upgrades");
		await expect(
			page.getByTestId("planned-upgrade-row").filter({ hasText: "kamaka" }),
		).toBeVisible();

		page.once("dialog", (dialog) => dialog.accept());
		await page.getByRole("button", { name: "Withdraw kamaka's plan" }).click();

		// Withdrawn, so it moves to the unplanned list and testing goes back to
		// aiming at the newest version.
		await expect(
			page.getByTestId("unplanned-upgrade-row").filter({ hasText: "kamaka" }),
		).toBeVisible();
		await expect(page.getByTestId("planned-upgrades")).toContainText(
			"No deployment has a recorded plan",
		);
	});
});
