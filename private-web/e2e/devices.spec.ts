import { expect, test } from "./test-fixtures";

test.describe("devices page", () => {
	test("loads with the search/untrusted/trusted tabs", async ({ page }) => {
		await page.goto("/devices");
		await expect(page.getByRole("tab", { name: "Search" })).toBeVisible();
		await expect(
			page.getByRole("tab", { name: "Untrusted devices" }),
		).toBeVisible();
		await expect(
			page.getByRole("tab", { name: "Trusted devices", exact: true }),
		).toBeVisible();
	});

	test("search input is on the index tab", async ({ page }) => {
		await page.goto("/devices");
		await expect(
			page.getByRole("searchbox", {
				name: /Search by public key/i,
			}),
		).toBeVisible();
	});

	test("untrusted tab navigates and renders rows or empty state", async ({
		page,
	}) => {
		await page.goto("/devices");
		await page.getByRole("tab", { name: "Untrusted devices" }).click();
		await expect(page).toHaveURL(/\/devices\/untrusted$/);
		await expect(
			page
				.locator(
					[
						'a[href^="/devices/"][href*="-"]',
						'[role="alert"]',
					].join(", "),
				)
				.first(),
		).toBeVisible();
	});
});

test.describe("device detail page", () => {
	test("loads with an id param", async ({ page }) => {
		await page.goto("/devices/00000000-0000-0000-0000-000000000000");
		await expect(
			page
				.locator(
					[
						'h1[class*="MuiTypography"]',
						'[role="alert"]',
					].join(", "),
				)
				.first(),
		).toBeVisible();
	});
});
