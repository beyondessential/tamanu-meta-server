import { expect, test } from "./test-fixtures";
import { resetSeededTables } from "./seed";

test.describe("calendar feeds", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("mints a subscription URL once, then revokes it", async ({ page }) => {
		await page.goto("/settings/calendar-feeds");
		await expect(page.getByText("No feeds minted.")).toBeVisible();

		await page.getByLabel("Feed name").fill("deployments team");
		await page.getByRole("button", { name: "Mint feed" }).click();

		const url = page.getByRole("dialog").getByRole("textbox");
		await expect(url).toHaveValue(/\/calendar\/canopy_cal_.+\/upgrades\.ics$/);
		await page.getByRole("button", { name: "Done" }).click();

		const row = page.getByTestId("calendar-feed-row");
		await expect(row).toContainText("deployments team");
		await expect(row).toContainText("never");
		await expect(row).toContainText("active");
		// The URL is shown once at minting and never again.
		await expect(row).not.toContainText("canopy_cal_");

		await page.getByRole("button", { name: "revoke deployments team" }).click();
		await page.getByRole("button", { name: "Revoke", exact: true }).click();
		await expect(row).toContainText("revoked");
	});
});
