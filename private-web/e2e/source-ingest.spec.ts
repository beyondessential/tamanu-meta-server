import { seedCheckPolicy } from "./seed";
import { expect, test } from "./test-fixtures";

test.describe("Source ingest gating", () => {
	test("setting ingest to deny persists and disables reachability", async ({
		page,
		sql,
	}) => {
		await seedCheckPolicy(sql, { source: "alertd", checkName: "db_connect" });
		await page.goto("/settings/healthchecks");

		const row = page.getByRole("row", { name: /alertd/ }).first();
		await expect(row).toBeVisible();

		await row.getByRole("button", { name: "deny", exact: true }).click();

		// Persists to the source policy...
		await expect
			.poll(async () => {
				const rows = await sql.query<{ ingest: string }>(
					`SELECT ingest FROM source_policies WHERE source = 'alertd'`,
				);
				return rows[0]?.ingest ?? null;
			})
			.toBe("deny");

		// ...and a non-allow source can't count for reachability, so the
		// reachability control is disabled.
		await expect(
			row.getByRole("button", { name: "on", exact: true }),
		).toBeDisabled();
	});
});
