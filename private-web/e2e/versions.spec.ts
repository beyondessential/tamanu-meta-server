import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedVersion } from "./seed";

test.describe("versions page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the seeded version under its minor heading", async ({
		page,
		sql,
	}) => {
		// Seed two patches on the same minor; the index groups them by minor.
		await seedVersion(sql, { major: 9, minor: 42, patch: 0 });
		await seedVersion(sql, { major: 9, minor: 42, patch: 1 });

		await page.goto("/versions");
		await expect(
			page.getByRole("heading", { name: "Versions", level: 1 }),
		).toBeVisible();

		// Group accordion summary uses `<major>.<minor>.<latest_patch>`.
		// 9.42.1 shows in both the summary and the expanded patch list, so
		// just ensure at least one rendering is visible.
		await expect(page.getByText("9.42.1").first()).toBeVisible();
	});
});

test.describe("version detail page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the changelog of the requested version", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, {
			major: 9,
			minor: 42,
			patch: 0,
			changelog: "fixed the foo for real this time",
		});

		await page.goto("/versions/9.42.0");

		// Version number appears as the page heading (monospace h1).
		await expect(
			page.getByRole("heading", { name: "9.42.0", level: 1 }),
		).toBeVisible();
		// And the changelog body renders.
		await expect(
			page.getByText("fixed the foo for real this time"),
		).toBeVisible();
	});
});
