import { seedCheckPolicy } from "./seed";
import { expect, test } from "./test-fixtures";

test.describe("Healthcheck decommissioning", () => {
	test("a gone-quiet check can be decommissioned from the catalog", async ({
		page,
		sql,
	}) => {
		// A catalogued check whose fleet-wide last report was 30 days ago:
		// past the 7-day window, so it shows as a decommissioning candidate.
		const longAgo = new Date(Date.now() - 30 * 24 * 3600 * 1000).toISOString();
		await seedCheckPolicy(sql, {
			source: "bestool-alertd",
			checkName: "fhir-queued-job-long",
			lastSeen: longAgo,
		});

		await page.goto("/settings/healthchecks");
		const row = page.getByRole("row", { name: /fhir-queued-job-long/ });
		await expect(row).toBeVisible();
		await expect(row.getByText("gone quiet")).toBeVisible();

		page.on("dialog", (dialog) => dialog.accept());
		await row.getByRole("button", { name: /decommission/i }).click();

		// The row flips to a decommissioned marker...
		await expect(
			page
				.getByRole("row", { name: /fhir-queued-job-long/ })
				.getByText("decommissioned"),
		).toBeVisible();

		// ...and the catalog row is marked in the database.
		await expect
			.poll(async () => {
				const rows = await sql.query<{ present: boolean }>(
					`SELECT decommissioned_at IS NOT NULL AS present FROM check_policies
					 WHERE source = 'bestool-alertd' AND check_name = 'fhir-queued-job-long'`,
				);
				return rows[0]?.present ?? false;
			})
			.toBe(true);
	});
});
