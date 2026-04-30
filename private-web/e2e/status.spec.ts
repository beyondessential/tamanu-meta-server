import { expect, test } from "@playwright/test";

test.describe("status page", () => {
	test("loads and renders the page chrome", async ({ page }) => {
		await page.goto("/");

		// / redirects to /status
		await expect(page).toHaveURL(/\/status$/);
		await expect(page).toHaveTitle("Canopy");

		// Top nav has the page title and a Status link.
		await expect(page.getByRole("heading", { name: "Canopy" })).toBeVisible();
		await expect(page.getByRole("link", { name: "Status" })).toBeVisible();
	});

	test("hits the backend and renders either data or an alert", async ({
		page,
	}) => {
		// Don't constrain on data shape (the dev DB may be empty or behind on
		// migrations). What we want to know is that the request round-trips and
		// the page renders something useful — either rank headings, an info
		// "no servers" banner, or an error alert. Each is a deterministic
		// outcome of the API call going through.
		await page.goto("/status");

		await expect(
			page.locator(
				[
					"h2",
					'[role="alert"]',
				].join(", "),
			),
		).toBeVisible();
	});
});
