import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedDevice,
	seedRestoreConsumerCapability,
	seedRestoreReplica,
	seedServerGroup,
	seedUpgradePlan,
	seedVersion,
	seedVersionKnownIssue,
} from "./seed";

/** Declare a replica on an intent that migrates, which is what makes a plan
 * something the pipeline will act on. */
async function declareUpgradeReplica(
	sql: Parameters<typeof seedRestoreReplica>[0],
	groupId: string,
): Promise<void> {
	const consumer = await seedDevice(sql, { role: "backup-restore" });
	await seedRestoreConsumerCapability(sql, {
		deviceId: consumer.id,
		intents: [{ intent: "upgrade", semantics: ["check", "once", "migrate"] }],
	});
	await seedRestoreReplica(sql, {
		consumerDeviceId: consumer.id,
		groupId,
		intent: "upgrade",
		name: "upgrade",
	});
}

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
		await declareUpgradeReplica(sql, planned.id);

		await page.goto("/upgrades");

		const plannedRow = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		await expect(plannedRow).toContainText("2.60.0");
		await expect(plannedRow).toContainText("2.61.0");
		await expect(page.getByTestId("planned-upgrade-note")).toContainText(
			"site can absorb 2.61 only",
		);
		await expect(plannedRow).toContainText("late");
		// The plan says where it is going; the verdict says whether the data
		// survives getting there. Untested until a consumer reports.
		await expect(plannedRow).toContainText("not yet tested");

		// The deployment with nothing recorded is the one this view exists to
		// surface, so it is listed rather than omitted, behind a disclosure.
		await page
			.getByRole("button", { name: "Show deployments with no plan" })
			.click();
		await expect(
			page
				.getByTestId("unplanned-upgrade-row")
				.filter({ hasText: "drifting" }),
		).toBeVisible();
		await expect(page.getByTestId("unplanned-upgrades")).not.toContainText(
			"kamaka",
		);
	});

	test("says a plan with nothing declared to test it is not set up", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		const target = await seedVersion(sql, { major: 2, minor: 61, patch: 0 });
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: target.id,
			plannedFor: "2020-01-01",
		});

		await page.goto("/upgrades");

		// Nothing is declared to migrate this group's data, so no test will ever
		// run: saying "not yet tested" would leave a reader waiting on a result
		// that cannot arrive.
		const row = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		await expect(row).toContainText("not set up");
		await expect(row).not.toContainText("not yet tested");
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
		await expect(page.getByTestId("planned-upgrades")).toContainText(
			"No deployment has a recorded plan",
		);
		await page
			.getByRole("button", { name: "Show deployments with no plan" })
			.click();
		await expect(
			page
				.getByTestId("unplanned-upgrade-row")
				.filter({ hasText: "kamaka" }),
		).toBeVisible();

		// The plan is kept, so where kamaka was going stays readable.
		await page.getByRole("button", { name: "Show past plans" }).click();
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
		await page.getByRole("button", { name: "Show past plans" }).click();

		const past = page.getByTestId("past-plan-row").filter({ hasText: "kamaka" });
		await expect(past).toHaveCount(1);
		await expect(past).toContainText("2.61.0");
		await expect(past).toContainText("replaced");
		await expect(page.getByTestId("past-plan-note")).toContainText(
			"site can absorb 2.61 only",
		);
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
		const note = page.getByTestId("planned-upgrade-note");
		await expect(note).toContainText("waiting on the site");

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

		await expect(note).toContainText("site confirmed the window");
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
			plannedTime: "02:00",
			plannedZone: "Pacific/Fiji",
		});

		await page.goto("/upgrades");
		const row = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		await expect(row).toContainText("late");

		await page.getByRole("button", { name: "Edit kamaka's plan" }).click();
		const dialog = page.getByTestId("edit-plan");
		await dialog.getByLabel("Planned for").fill("");
		// The hour was an hour of that day, so it goes with it.
		await expect(dialog.getByLabel("Time", { exact: true })).toHaveValue("");
		await dialog.getByRole("button", { name: "Save" }).click();

		// No date means nothing to be late against.
		await expect(row).not.toContainText("late");
		await expect(row).not.toContainText("FJT");
	});

	test("a version far behind the newest can still be planned", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.53.0' WHERE id = $1",
			[group.id],
		);
		// Ten minors of patch releases sit between the deployment and the newest, so the
		// version it is going to is a long way down the list.
		await seedVersion(sql, { major: 2, minor: 54, patch: 0 });
		for (let minor = 56; minor <= 65; minor++) {
			for (let patch = 0; patch <= 5; patch++) {
				await seedVersion(sql, { major: 2, minor, patch });
			}
		}

		await page.goto("/upgrades");
		const form = page.getByTestId("record-plan");
		await form.getByLabel("Deployment").click();
		await page.getByRole("option", { name: "kamaka" }).click();

		// Unfiltered, the list is the newest patch of each of the last ten minors:
		// short enough to scroll, so the hundreds of plannable patches don't have
		// to be.
		await form.getByLabel("Going to").click();
		await expect(page.getByRole("option")).toHaveCount(10);
		await expect(page.getByRole("option").first()).toHaveText("2.65.5");
		await expect(page.getByRole("option", { name: "2.54.0" })).toHaveCount(0);

		await form.getByLabel("Going to").fill("2.54");
		await page.getByRole("option", { name: "2.54.0" }).click();
		await form.getByRole("button", { name: "Record" }).click();

		await expect(
			page.getByTestId("planned-upgrade-row").filter({ hasText: "kamaka" }),
		).toContainText("2.54.0");
	});

	test("a minor is suggested at its newest patch clear of known issues", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = $1",
			[group.id],
		);
		await seedVersion(sql, { major: 2, minor: 61, patch: 0 });
		await seedVersion(sql, { major: 2, minor: 61, patch: 1 });
		await seedVersion(sql, { major: 2, minor: 61, patch: 2 });
		await seedVersionKnownIssue(sql, { major: 2, minor: 61, patch: 2 });

		await page.goto("/upgrades");
		const form = page.getByTestId("record-plan");
		await form.getByLabel("Deployment").click();
		await page.getByRole("option", { name: "kamaka" }).click();
		await form.getByLabel("Going to").click();

		// The newest patch carries the issue, so the minor is suggested one back.
		await expect(page.getByRole("option")).toHaveCount(1);
		await expect(page.getByRole("option")).toHaveText("2.61.1");

		// Still reachable by typing, and marked: an issue may be resolved well
		// before the planned date.
		await form.getByLabel("Going to").fill("2.61.2");
		await expect(
			page.getByRole("option").filter({ hasText: "2.61.2" }),
		).toContainText("known issue");
	});

	test("the hour an upgrade starts reads beside the day", async ({
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
			plannedTime: "00:00",
			plannedZone: "Pacific/Fiji",
		});

		await page.goto("/upgrades");
		const row = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		// Whose midnight it is, without the reader having to know the deployment.
		await expect(row).toContainText("12am FJT");

		await page.getByRole("button", { name: "Edit kamaka's plan" }).click();
		const dialog = page.getByTestId("edit-plan");
		await expect(dialog.getByLabel("Time", { exact: true })).toHaveValue(
			"00:00",
		);
		await expect(dialog.getByLabel("Timezone")).toHaveValue("Pacific/Fiji");

		await dialog.getByLabel("Time", { exact: true }).fill("19:30");
		await dialog.getByLabel("Timezone").fill("Pacific/Nauru");
		await page.getByRole("option", { name: "Pacific/Nauru" }).click();
		await dialog.getByRole("button", { name: "Save" }).click();

		await expect(row).toContainText("7:30pm NRT");
	});

	test("a plan can be recorded with an hour, and the hour taken back off", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = $1",
			[group.id],
		);
		await seedVersion(sql, { major: 2, minor: 61, patch: 0 });

		await page.goto("/upgrades");
		const form = page.getByTestId("record-plan");
		// Nothing to say about a deployment until one is named.
		await expect(form.getByLabel("Planned for")).toBeDisabled();
		await expect(form.getByLabel("Note")).toBeDisabled();

		await form.getByLabel("Deployment").click();
		await page.getByRole("option", { name: "kamaka" }).click();
		await form.getByLabel("Going to").click();
		await page.getByRole("option", { name: "2.61.0" }).click();
		await form.getByLabel("Planned for").fill("2030-04-05");
		await form.getByLabel("Time", { exact: true }).fill("23:00");
		// Fiji is where most of the fleet is, so it stands unless changed.
		await expect(form.getByLabel("Timezone")).toHaveValue("Pacific/Fiji");
		await form.getByRole("button", { name: "Record" }).click();

		const row = page
			.getByTestId("planned-upgrade-row")
			.filter({ hasText: "kamaka" });
		await expect(row).toContainText("11pm FJT");

		// An hour that is no longer settled comes off without losing the day.
		await page.getByRole("button", { name: "Edit kamaka's plan" }).click();
		const dialog = page.getByTestId("edit-plan");
		await dialog.getByLabel("Time", { exact: true }).fill("");
		await dialog.getByRole("button", { name: "Save" }).click();

		await expect(row).toContainText("2030-04-05");
		await expect(row).not.toContainText("FJT");
	});
});

test.describe("upgrade calendar", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("shows a dated plan on the day it lands", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "kamaka" });
		await sql.query(
			"UPDATE server_groups SET effective_version = '2.60.0' WHERE id = $1",
			[group.id],
		);
		const target = await seedVersion(sql, { major: 2, minor: 61, patch: 0 });

		const now = new Date();
		const day = new Date(now.getFullYear(), now.getMonth(), 15);
		const iso = [
			day.getFullYear(),
			String(day.getMonth() + 1).padStart(2, "0"),
			"15",
		].join("-");
		await seedUpgradePlan(sql, {
			groupId: group.id,
			targetVersionId: target.id,
			plannedFor: iso,
		});

		await page.goto("/upgrades");

		const cell = page
			.getByTestId("upgrade-calendar")
			.getByTestId("calendar-day")
			.filter({ has: page.locator(`[data-testid="calendar-entry"]`) });
		await expect(cell).toHaveAttribute("data-date", iso);
		await expect(cell).toContainText("kamaka 2.61.0");

		// The month the reader is looking at is named, and moves.
		await expect(page.getByTestId("upgrade-calendar")).toContainText(
			now.toLocaleDateString(undefined, { month: "long", year: "numeric" }),
		);
		await page.getByRole("button", { name: "next month" }).click();
		await expect(page.getByTestId("calendar-entry")).toHaveCount(0);
		await page.getByRole("button", { name: "Today" }).click();
		await expect(page.getByTestId("calendar-entry")).toHaveCount(1);
	});
});
