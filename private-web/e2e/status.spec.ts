import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedServer, seedVersion } from "./seed";

test.describe("status page", () => {
	test.beforeEach(async ({ sql }) => {
		// Each test inside this file makes assertions about either
		// emptiness or specific seeded rows, so wipe state on entry.
		// Worker-scoped fixtures otherwise leak between tests/specs.
		await resetSeededTables(sql);
	});

	test("renders the nav and redirects /", async ({ page }) => {
		await page.goto("/");
		await expect(page).toHaveURL(/\/status$/);
		await expect(page).toHaveTitle("Canopy");
		await expect(page.getByRole("link", { name: "Status" })).toBeVisible();
	});

	test("empty database surfaces an info banner", async ({ page }) => {
		await page.goto("/status");
		await expect(page.getByText(/No servers configured/i)).toBeVisible();
	});

	test("groups centrals by rank and links to their detail page", async ({
		page,
		sql,
	}) => {
		// server_details computes version-distance against the latest
		// published version; without one it 404s the card.
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const prodCentral = await seedServer(sql, {
			name: "prod-alpha",
			kind: "central",
			rank: "production",
		});
		await seedServer(sql, {
			name: "demo-beta",
			kind: "central",
			rank: "demo",
		});

		await page.goto("/status");

		// Rank section headings.
		await expect(
			page.getByRole("heading", { name: "production", exact: true }),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "demo", exact: true }),
		).toBeVisible();

		// Server names render as h3 inside each card.
		await expect(
			page.getByRole("heading", { name: prodCentral.name }),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "demo-beta" }),
		).toBeVisible();

		// Each card is wrapped in a router link to /servers/<id>.
		await expect(
			page.locator(`a[href="/servers/${prodCentral.id}"]`),
		).toBeVisible();
	});
});
