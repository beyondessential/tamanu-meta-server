import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedManualIncident,
	seedServerGroup,
} from "./seed";

test.describe("manual incidents", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("incidents page lists manual incidents with chips", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "manual-inc-group" });
		const ongoing = await seedManualIncident(sql, {
			title: "Sync stuck in Wonderland",
			serverGroupId: group.id,
			startedAt: new Date(Date.now() - 60 * 60_000).toISOString(),
		});
		const ended = await seedManualIncident(sql, {
			title: "Yesterday's outage",
			description: "**bold** impact",
			startedAt: new Date(Date.now() - 26 * 3_600_000).toISOString(),
			endedAt: new Date(Date.now() - 24 * 3_600_000).toISOString(),
		});

		await page.goto("/incidents");
		await expect(
			page.getByRole("heading", { level: 2, name: "Manual incidents" }),
		).toBeVisible();

		const ongoingCard = page.getByRole("link", { name: ongoing.title });
		await expect(ongoingCard).toBeVisible();
		await expect(ongoingCard.getByText("manual", { exact: true })).toBeVisible();
		await expect(ongoingCard.getByText("ongoing", { exact: true })).toBeVisible();
		await expect(ongoingCard.getByText(group.name)).toBeVisible();

		const endedCard = page.getByRole("link", { name: ended.title });
		await expect(endedCard).toBeVisible();
		await expect(endedCard.getByText("manual", { exact: true })).toBeVisible();
		await expect(
			endedCard.getByText("ongoing", { exact: true }),
		).not.toBeVisible();
		await expect(endedCard.getByText("Fleet-wide")).toBeVisible();
	});

	test("clicking a card opens the detail page", async ({ page, sql }) => {
		const incident = await seedManualIncident(sql, {
			title: "Backups delayed",
			description: "**bold** impact on nightly runs",
			startedAt: new Date(Date.now() - 2 * 3_600_000).toISOString(),
			endedAt: new Date(Date.now() - 3_600_000).toISOString(),
			createdBy: "support@bes.au",
		});

		await page.goto("/incidents");
		await page.getByRole("link", { name: incident.title }).click();

		await expect(page).toHaveURL(`/incidents/manual/${incident.id}`);
		await expect(
			page.getByRole("heading", { level: 1, name: incident.title }),
		).toBeVisible();
		await expect(page.getByText("manual", { exact: true })).toBeVisible();
		await expect(page.getByText("ended", { exact: true })).toBeVisible();
		await expect(
			page.getByText("ongoing", { exact: true }),
		).not.toBeVisible();
		// The markdown description renders (not the raw `**bold**` source).
		await expect(
			page.locator("strong", { hasText: "bold" }),
		).toBeVisible();
		await expect(page.getByText("**bold**")).not.toBeVisible();
		await expect(page.getByText("recorded by support@bes.au")).toBeVisible();
	});

	test("section is absent when no manual incidents exist", async ({
		page,
	}) => {
		await page.goto("/incidents");
		// Wait for the page to settle on its (empty) incidents state first,
		// so the absence check isn't passing vacuously against a blank page.
		await expect(page.getByText("No open incidents.")).toBeVisible();
		await expect(
			page.getByRole("heading", { level: 2, name: "Manual incidents" }),
		).not.toBeVisible();
	});
});
