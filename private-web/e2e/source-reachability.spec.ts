import { resetSeededTables, seedCheckPolicy } from "./seed";
import { expect, test } from "./test-fixtures";

const SOURCES_PATH = "/settings/healthchecks/sources";

test.describe("Source reachability", () => {
	// Each test asserts against a source's starting policy, and a policy set
	// here outlives the test that set it — the stack (and its database) is
	// per worker, not per test.
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("an operator can change a source's reachability mode", async ({
		page,
		sql,
	}) => {
		// A source appears in the list once it has a catalogued check.
		await seedCheckPolicy(sql, { source: "alertd", checkName: "db_connect" });

		await page.goto(SOURCES_PATH);

		// The Sources table lists alertd; its reachability toggle starts "on".
		const row = page.getByRole("row", { name: /alertd/ }).first();
		await expect(row).toBeVisible();
		await expect(
			row.getByRole("button", { name: "on", exact: true }),
		).toHaveAttribute("aria-pressed", "true");

		await row.getByRole("button", { name: "quiet", exact: true }).click();

		// The change is confirmed in a dialog naming the target mode and
		// spelling out its consequence before it applies.
		const dialog = page.getByRole("dialog");
		await expect(dialog).toContainText(/set alertd reachability to .quiet./i);
		await expect(dialog).toContainText(/no longer raise a warning/i);
		await dialog.getByRole("button", { name: /confirm/i }).click();

		// The change persists to the source policy.
		await expect
			.poll(async () => {
				const rows = await sql.query<{ reachability: string }>(
					`SELECT reachability FROM source_policies WHERE source = 'alertd'`,
				);
				return rows[0]?.reachability ?? null;
			})
			.toBe("quiet");
	});

	test("cancelling the confirmation leaves the mode untouched", async ({
		page,
		sql,
	}) => {
		await seedCheckPolicy(sql, { source: "alertd", checkName: "db_connect" });
		await page.goto(SOURCES_PATH);

		const row = page.getByRole("row", { name: /alertd/ }).first();
		await row.getByRole("button", { name: "off", exact: true }).click();

		const dialog = page.getByRole("dialog");
		await expect(dialog).toBeVisible();
		await dialog.getByRole("button", { name: /cancel/i }).click();

		// The toggle stays on its original "on", and nothing is written.
		await expect(
			row.getByRole("button", { name: "on", exact: true }),
		).toHaveAttribute("aria-pressed", "true");
		const rows = await sql.query<{ reachability: string }>(
			`SELECT reachability FROM source_policies WHERE source = 'alertd'`,
		);
		// No policy row is written by a cancelled change (a source with no
		// explicit policy defaults to "on").
		expect(rows[0]?.reachability ?? "on").toBe("on");
	});
});
