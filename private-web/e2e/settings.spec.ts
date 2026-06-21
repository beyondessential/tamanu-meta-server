import { expect, test } from "./test-fixtures";

test.describe("Settings", () => {
	test("groups Admins + Recovery vault under one nav item", async ({ page }) => {
		await page.goto("/status");

		// The top bar has a single "Settings" item (no standalone Admins /
		// Recovery vault). Following it lands on Admins (the index).
		await page.getByRole("link", { name: "Settings" }).click();
		await expect(page).toHaveURL(/\/settings\/admins$/);
		await expect(page.getByRole("tab", { name: "Admins" })).toBeVisible();

		// The Recovery vault tab switches to its page.
		await page.getByRole("tab", { name: "Recovery vault" }).click();
		await expect(page).toHaveURL(/\/settings\/recovery$/);
		await expect(
			page.getByRole("heading", { name: /recovery vault/i }),
		).toBeVisible();
	});

	test("backup defaults editor edits the canopy-wide per-type default", async ({
		page,
		sql,
	}) => {
		await page.goto("/settings/backup-defaults");
		// The seeded tamanu-postgres default is shown.
		await expect(page.getByText("tamanu-postgres")).toBeVisible();

		await page.getByLabel("Back up every (hours)").fill("8");
		await page.getByRole("button", { name: /^save$/i }).click();

		await expect
			.poll(async () => {
				const rows = await sql.query<{ secs: string }>(
					`SELECT EXTRACT(EPOCH FROM default_interval)::text AS secs
					 FROM backup_type_defaults WHERE type = 'tamanu-postgres'`,
				);
				return rows[0] ? Number(rows[0].secs) : null;
			})
			.toBe(28800);
	});
});
