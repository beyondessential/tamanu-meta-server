import { seedCheckPolicy } from "./seed";
import { expect, test } from "./test-fixtures";

test.describe("Source reachability", () => {
	test("an operator can change a source's reachability mode", async ({
		page,
		sql,
	}) => {
		// A source appears in the list once it has a catalogued check.
		await seedCheckPolicy(sql, { source: "alertd", checkName: "db_connect" });

		await page.goto("/settings/healthchecks");

		// The Sources table lists alertd; its reachability toggle starts "on".
		const row = page.getByRole("row", { name: /alertd/ }).first();
		await expect(row).toBeVisible();
		await expect(
			row.getByRole("button", { name: "on", exact: true }),
		).toHaveAttribute("aria-pressed", "true");

		await row.getByRole("button", { name: "quiet", exact: true }).click();

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
});
