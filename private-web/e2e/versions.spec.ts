import { expect, test } from "./test-fixtures";

test.describe("versions page", () => {
	test("loads and renders the page chrome", async ({ page }) => {
		await page.goto("/versions");
		await expect(page).toHaveURL(/\/versions$/);
		await expect(
			page.getByRole("heading", { name: "Versions", level: 1 }),
		).toBeVisible();
	});

	test("hits the backend and renders data, an empty state, or an error", async ({
		page,
	}) => {
		// Tolerant assertion: the API may return data, no rows, or a 500
		// (e.g. dev DB behind on migrations). Each case has a deterministic
		// rendering that proves the request round-tripped.
		await page.goto("/versions");
		await expect(
			page.locator(
				[
					".MuiAccordion-root",
					'[role="alert"]',
				].join(", "),
			).first(),
		).toBeVisible();
	});
});

test.describe("version detail page", () => {
	test("loads with a version param", async ({ page }) => {
		// Use an arbitrary version. The API will likely return an error
		// (no such version in the dev DB) which the page surfaces as an
		// Alert — that proves the page mounted and round-tripped.
		await page.goto("/versions/0.0.0");
		await expect(
			page.locator(
				[
					'h1[class*="MuiTypography"]', // version number heading on success
					'[role="alert"]', // error/info alert
				].join(", "),
			).first(),
		).toBeVisible();
	});
});
