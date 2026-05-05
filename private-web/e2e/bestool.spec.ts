import { expect, test } from "./test-fixtures";

test.describe("bestool snippets list", () => {
	test("/bestool redirects to /bestool/snippets and renders the page", async ({
		page,
	}) => {
		await page.goto("/bestool");
		await expect(page).toHaveURL(/\/bestool\/snippets$/);
		await expect(
			page.getByRole("heading", { name: "PSQL Snippets" }),
		).toBeVisible();
		await expect(page.getByRole("button", { name: "Add" })).toBeVisible();
	});

	test("Add toggles the create form", async ({ page }) => {
		await page.goto("/bestool/snippets");
		await page.getByRole("button", { name: "Add" }).click();
		await expect(page.getByLabel(/^Name/)).toBeVisible();
		await page.getByRole("button", { name: "Cancel" }).click();
		await expect(page.getByLabel(/^Name/)).not.toBeVisible();
	});
});
