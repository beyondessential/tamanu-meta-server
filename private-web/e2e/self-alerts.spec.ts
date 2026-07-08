import { resetSeededTables, seedIssue } from "./seed";
import { expect, test } from "./test-fixtures";

const NIL = "00000000-0000-0000-0000-000000000000";

// Self-alerts (canopy-wide issues, scoped to neither server nor group)
// get their own surface: a banner on every page and the /alerts view.
// They must NOT appear in the fleet issue listing on /incidents.
test.describe("self-alerts", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("active alert banners on any page and lists on /alerts", async ({
		page,
		sql,
	}) => {
		await seedIssue(sql, {
			source: "canopy",
			ref: "mcp-token-expiry",
			severity: "error",
			description: "MCP access token nearing expiry",
			message: "token claude expires in 10 days",
		});

		await page.goto("/status");
		const banner = page.getByText("Canopy: MCP access token nearing expiry");
		await expect(banner).toBeVisible();

		await page.getByRole("link", { name: "details" }).click();
		await expect(page).toHaveURL(/\/alerts$/);
		await expect(
			page.getByText("token claude expires in 10 days"),
		).toBeVisible();
	});

	test("self-alerts stay out of the fleet issue listing", async ({
		page,
		sql,
	}) => {
		await seedIssue(sql, {
			source: "canopy",
			ref: "preflight-identity",
			severity: "critical",
			description: "Canopy IRSA identity broken",
			message: "sts:GetCallerIdentity failed",
		});

		await page.goto("/incidents?showAll=1");
		await expect(
			page.getByText("sts:GetCallerIdentity failed"),
		).not.toBeVisible();
		// And no link to the hidden nil-server page anywhere.
		await expect(page.locator(`a[href="/servers/${NIL}"]`)).toHaveCount(0);
	});

	test("resolve clears the banner", async ({ page, sql }) => {
		await seedIssue(sql, {
			source: "canopy",
			ref: "slack-delivery-failure",
			severity: "error",
			description: "Slack delivery permanently failed",
			message: "outbox row gave up after 10 attempts",
		});

		await page.goto("/alerts");
		await expect(
			page.getByText("outbox row gave up after 10 attempts"),
		).toBeVisible();
		await page.getByRole("button", { name: "Resolve" }).click();

		await expect(page.getByText("No active alerts.")).toBeVisible();
		await expect(
			page.getByText("Canopy: Slack delivery permanently failed"),
		).not.toBeVisible();
		// The resolved alert stays visible as history.
		await expect(page.getByText("Recovered and resolved")).toBeVisible();
	});
});
