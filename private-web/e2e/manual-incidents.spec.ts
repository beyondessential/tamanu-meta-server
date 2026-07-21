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
		const other = await seedServerGroup(sql, { name: "other-inc-group" });
		const ongoing = await seedManualIncident(sql, {
			title: "Sync stuck in Wonderland",
			serverGroupId: group.id,
			startedAt: new Date(Date.now() - 60 * 60_000).toISOString(),
		});
		const ended = await seedManualIncident(sql, {
			title: "Yesterday's outage",
			description: "**bold** impact",
			serverGroupId: other.id,
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
		await expect(endedCard.getByText(other.name)).toBeVisible();
	});

	test("clicking a card opens the detail page", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "detail-group" });
		const incident = await seedManualIncident(sql, {
			title: "Backups delayed",
			description: "**bold** impact on nightly runs",
			serverGroupId: group.id,
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
		// The affected group links through to its page.
		await expect(
			page.getByRole("link", { name: group.name }),
		).toBeVisible();
		// The markdown description renders (not the raw `**bold**` source).
		await expect(
			page.locator("strong", { hasText: "bold" }),
		).toBeVisible();
		await expect(page.getByText("**bold**")).not.toBeVisible();
		await expect(page.getByText("recorded by support@bes.au")).toBeVisible();
	});

	test("recording a manual incident from the incidents page", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "record-group" });

		await page.goto("/incidents");
		// The section (with its record button) is present even with nothing
		// recorded yet; only the cards are absent.
		await expect(
			page.getByRole("heading", { level: 2, name: "Manual incidents" }),
		).toBeVisible();

		await page.getByRole("button", { name: "Record incident" }).click();
		await page.getByLabel("Title").fill("Fibre cut in Suva");
		await page.getByLabel("Affected group").click();
		await page.getByRole("option", { name: group.name }).click();
		await page
			.getByLabel("Description (markdown)")
			.fill("ISP outage took the site offline.");
		await page.getByRole("button", { name: "Record", exact: true }).click();

		const card = page.getByRole("link", { name: "Fibre cut in Suva" });
		await expect(card).toBeVisible();
		await expect(card.getByText("ongoing", { exact: true })).toBeVisible();
		await expect(card.getByText(group.name)).toBeVisible();
		// Attributed to the dev-bypass tailnet user.
		await expect(card.getByText("by admin@localhost")).toBeVisible();
	});

	test("editing a manual incident from the detail page", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "edit-group" });
		const other = await seedServerGroup(sql, { name: "moved-to-group" });
		const incident = await seedManualIncident(sql, {
			title: "Original title",
			serverGroupId: group.id,
			startedAt: new Date(Date.now() - 2 * 3_600_000).toISOString(),
		});

		await page.goto(`/incidents/manual/${incident.id}`);
		await page.getByRole("button", { name: "Edit" }).click();
		await page.getByLabel("Title").fill("Corrected title");
		await page.getByLabel("Affected group").click();
		await page.getByRole("option", { name: other.name }).click();
		await page
			.getByLabel("Ended (empty while ongoing)")
			.fill("2026-07-01T12:30");
		await page.getByRole("button", { name: "Save" }).click();

		await expect(
			page.getByRole("heading", { level: 1, name: "Corrected title" }),
		).toBeVisible();
		await expect(page.getByText("ended", { exact: true })).toBeVisible();
		await expect(
			page.getByRole("link", { name: other.name }),
		).toBeVisible();
	});

	test("deleting a manual incident returns to the incidents page", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "delete-group" });
		const incident = await seedManualIncident(sql, {
			title: "To be removed",
			serverGroupId: group.id,
		});

		await page.goto(`/incidents/manual/${incident.id}`);
		await page.getByRole("button", { name: "Delete" }).click();
		await page
			.getByRole("dialog")
			.getByRole("button", { name: "Delete" })
			.click();

		await expect(page).toHaveURL("/incidents");
		await expect(
			page.getByRole("link", { name: incident.title }),
		).not.toBeVisible();
	});
});
