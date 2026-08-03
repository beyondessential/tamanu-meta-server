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
		const target = await seedVersion(sql, {
			major: 2,
			minor: 61,
			patch: 0,
		});

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
			page
				.getByTestId("unplanned-upgrade-row")
				.filter({ hasText: "drifting" }),
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
		const target = await seedVersion(sql, {
			major: 2,
			minor: 61,
			patch: 0,
		});
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: target.id,
		});

		await page.goto("/upgrades");
		await expect(
			page
				.getByTestId("planned-upgrade-row")
				.filter({ hasText: "kamaka" }),
		).toBeVisible();

		page.once("dialog", (dialog) => dialog.accept());
		await page
			.getByRole("button", { name: "Withdraw kamaka's plan" })
			.click();

		// Withdrawn, so it moves to the unplanned list and stops being tested.
		await expect(
			page
				.getByTestId("unplanned-upgrade-row")
				.filter({ hasText: "kamaka" }),
		).toBeVisible();
		await expect(page.getByTestId("planned-upgrades")).toContainText(
			"No deployment has a recorded plan",
		);

		// The plan is kept, so where kamaka was going stays readable.
		const past = page
			.getByTestId("past-plan-row")
			.filter({ hasText: "kamaka" });
		await expect(past).toContainText("2.61.0");
		await expect(past).toContainText("withdrawn");
	});

	test("a plan replaced by a later one reads as replaced, not withdrawn", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = $1",
			[group.id],
		);
		const first = await seedVersion(sql, { major: 2, minor: 61, patch: 0 });
		const second = await seedVersion(sql, { major: 2, minor: 63, patch: 0 });
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: first.id,
			note: "site can absorb 2.61 only",
		});
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: second.id,
			supersedes: true,
		});

		await page.goto("/upgrades");

		const past = page.getByTestId("past-plan-row").filter({ hasText: "kamaka" });
		await expect(past).toHaveCount(1);
		await expect(past).toContainText("2.61.0");
		await expect(past).toContainText("replaced");
		await expect(past).toContainText("site can absorb 2.61 only");
		// The plan that replaced it is where the deployment is going, so it stays
		// out of the history.
		await expect(
			page.getByTestId("planned-upgrade-row").filter({ hasText: "kamaka" }),
		).toContainText("2.63.0");
	});

	test("amending a plan changes its date and note but not where it is going", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = $1",
			[group.id],
		);
		const target = await seedVersion(sql, {
			major: 2,
			minor: 61,
			patch: 0,
		});
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: target.id,
			plannedFor: "2020-01-01",
			note: "waiting on the site",
		});

		await page.goto("/upgrades");
		const row = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		await expect(row).toContainText("waiting on the site");

		await page.getByRole("button", { name: "Edit kamaka's plan" }).click();
		const dialog = page.getByTestId("edit-plan");
		// Prefilled from the plan, so an operator amends rather than retypes.
		await expect(dialog.getByLabel("Planned for")).toHaveValue(
			"2020-01-01",
		);
		await expect(dialog.getByLabel("Note")).toHaveValue(
			"waiting on the site",
		);

		await dialog.getByLabel("Planned for").fill("2020-03-03");
		await dialog.getByLabel("Note").fill("site confirmed the window");
		await dialog.getByRole("button", { name: "Save" }).click();

		await expect(row).toContainText("site confirmed the window");
		await expect(row).toContainText("2020-03-03");
		// Same plan, same destination: amending is not a replacement.
		await expect(row).toContainText("2.61.0");
		await expect(page.getByTestId("planned-upgrade-row")).toHaveCount(1);
	});

	test("a date no longer expected can be cleared", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = $1",
			[group.id],
		);
		const target = await seedVersion(sql, {
			major: 2,
			minor: 61,
			patch: 0,
		});
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: target.id,
			plannedFor: "2020-01-01",
		});

		await page.goto("/upgrades");
		const row = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		await expect(row).toContainText("late");

		await page.getByRole("button", { name: "Edit kamaka's plan" }).click();
		const dialog = page.getByTestId("edit-plan");
		await dialog.getByLabel("Planned for").fill("");
		await dialog.getByRole("button", { name: "Save" }).click();

		// No date means nothing to be late against.
		await expect(row).not.toContainText("late");
	});
});
