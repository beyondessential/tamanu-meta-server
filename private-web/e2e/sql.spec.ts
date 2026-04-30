import { expect, test } from "@playwright/test";

test.describe("sql page", () => {
	test("loads with the page chrome and a query box", async ({ page }) => {
		await page.goto("/sql");
		await expect(
			page.getByRole("heading", { name: "SQL Playground" }),
		).toBeVisible();
		await expect(page.getByLabel("Query")).toBeVisible();
		await expect(page.getByRole("button", { name: "Run" })).toBeDisabled();
	});

	test("Run is enabled when the query is non-empty", async ({ page }) => {
		await page.goto("/sql");
		await page.getByLabel("Query").fill("SELECT 1");
		await expect(page.getByRole("button", { name: "Run" })).toBeEnabled();
	});
});
