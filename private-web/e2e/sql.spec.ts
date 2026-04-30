import { expect, test } from "@playwright/test";

test.describe("sql page", () => {
	test("loads with the page chrome and a query box", async ({ page }) => {
		await page.goto("/sql");
		await expect(
			page.getByRole("heading", { name: "SQL Playground" }),
		).toBeVisible();
		await expect(page.locator(".cm-editor")).toBeVisible();
		await expect(page.getByRole("button", { name: "Run" })).toBeDisabled();
	});

	test("Run is enabled when the query is non-empty", async ({ page }) => {
		await page.goto("/sql");
		await page.locator(".cm-content").click();
		await page.keyboard.type("SELECT 1");
		await expect(page.getByRole("button", { name: "Run" })).toBeEnabled();
	});
});
