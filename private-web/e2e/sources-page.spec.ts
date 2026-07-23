import { seedCheckPolicy } from "./seed";
import { expect, test } from "./test-fixtures";

test.describe("Sources page", () => {
	test("the healthcheck catalog links out to the sources page", async ({
		page,
		sql,
	}) => {
		await seedCheckPolicy(sql, { source: "alertd", checkName: "db_connect" });

		// The catalog page no longer embeds the sources table — it offers a
		// link out to the dedicated page instead.
		await page.goto("/settings/healthchecks");
		await expect(
			page.getByRole("columnheader", { name: "Reachability" }),
		).toHaveCount(0);

		await page.getByRole("link", { name: /manage sources/i }).click();

		await expect(page).toHaveURL(/\/settings\/healthchecks\/sources$/);
		await expect(page.getByRole("heading", { name: "Sources" })).toBeVisible();
		await expect(page.getByRole("row", { name: /alertd/ }).first()).toBeVisible();

		// The Settings tab bar keeps Healthchecks selected on the sub-page.
		await expect(page.getByRole("tab", { name: "Healthchecks" })).toHaveAttribute(
			"aria-selected",
			"true",
		);

		// A back link returns to the catalog.
		await page.getByRole("link", { name: /all healthchecks/i }).click();
		await expect(page).toHaveURL(/\/settings\/healthchecks$/);
	});
});
