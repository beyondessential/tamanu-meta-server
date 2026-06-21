import { expect, test } from "./test-fixtures";

test.describe("Settings", () => {
	test("groups Admins + Recovery vault under one nav item", async ({ page }) => {
		await page.goto("/status");

		// The top bar has a single "Settings" item (no standalone Admins /
		// Recovery vault). Following it lands on Admins (the index).
		await page.getByRole("link", { name: "Settings" }).click();
		await expect(page).toHaveURL(/\/settings\/admins$/);
		await expect(page.getByRole("tab", { name: "Admins" })).toBeVisible();

		// The Recovery vault tab switches to its page.
		await page.getByRole("tab", { name: "Recovery vault" }).click();
		await expect(page).toHaveURL(/\/settings\/recovery$/);
		await expect(
			page.getByRole("heading", { name: /recovery vault/i }),
		).toBeVisible();
	});
});
