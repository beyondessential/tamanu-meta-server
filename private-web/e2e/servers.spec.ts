import { expect, test } from "@playwright/test";

test.describe("servers list page", () => {
	test("loads and shows the central/facility tabs", async ({ page }) => {
		await page.goto("/servers");
		await expect(page.getByRole("tab", { name: "Central servers" })).toHaveAttribute(
			"aria-selected",
			"true",
		);
		await expect(
			page.getByRole("tab", { name: "Facility servers" }),
		).toBeVisible();
	});

	test("switches to facilities tab on click", async ({ page }) => {
		await page.goto("/servers");
		await page.getByRole("tab", { name: "Facility servers" }).click();
		await expect(page).toHaveURL(/\/servers\/facilities$/);
		await expect(
			page.getByRole("tab", { name: "Facility servers" }),
		).toHaveAttribute("aria-selected", "true");
	});

	test("hits the backend and renders rows or an error", async ({ page }) => {
		await page.goto("/servers");
		await expect(
			page
				.locator(
					[
						'a[href^="/servers/"][href*="-"]', // server row link with UUID
						'[role="alert"]',
					].join(", "),
				)
				.first(),
		).toBeVisible();
	});
});

test.describe("server detail page", () => {
	test("loads with an id param", async ({ page }) => {
		// Use an arbitrary UUID. The API likely 404s; we just want to know
		// the page mounts and surfaces a deterministic state.
		await page.goto("/servers/00000000-0000-0000-0000-000000000000");
		await expect(
			page
				.locator(
					[
						'h1[class*="MuiTypography"]', // server name heading on success
						'[role="alert"]',
					].join(", "),
				)
				.first(),
		).toBeVisible();
	});
});

test.describe("server edit page", () => {
	test("loads with an id param", async ({ page }) => {
		await page.goto("/servers/00000000-0000-0000-0000-000000000000/edit");
		await expect(
			page
				.locator(
					[
						"form", // edit form mounts on success
						'[role="alert"]',
					].join(", "),
				)
				.first(),
		).toBeVisible();
	});
});
