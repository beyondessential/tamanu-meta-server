import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServer,
	seedServerGroup,
	seedVersion,
} from "./seed";

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
		await expect(page.getByText(/no server groups configured/i)).toBeVisible();
	});

	test("groups by rank bucket and links to each group's detail page", async ({
		page,
		sql,
	}) => {
		// group_details computes version-distance against the latest
		// published version; without one the card 404s.
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const prodGroup = await seedServerGroup(sql, { name: "prod-cluster" });
		const demoGroup = await seedServerGroup(sql, { name: "demo-cluster" });
		await seedServer(sql, {
			name: "prod-alpha",
			kind: "central",
			rank: "production",
			groupId: prodGroup.id,
		});
		await seedServer(sql, {
			name: "demo-beta",
			kind: "central",
			rank: "demo",
			groupId: demoGroup.id,
		});

		await page.goto("/status");

		// Rank section headings.
		await expect(
			page.getByRole("heading", { name: "production", exact: true }),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "demo", exact: true }),
		).toBeVisible();

		// Group names render as h3 inside each card.
		await expect(
			page.getByRole("heading", { name: prodGroup.name }),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: demoGroup.name }),
		).toBeVisible();

		// Each card is wrapped in a router link to /groups/<id>.
		await expect(
			page.locator(`a[href="/groups/${prodGroup.id}"]`),
		).toBeVisible();
	});
});
