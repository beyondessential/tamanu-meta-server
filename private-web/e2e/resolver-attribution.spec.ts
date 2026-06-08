import {
	resetSeededTables,
	seedIssue,
	seedServer,
	seedServerGroup,
	seedTailscaleUser,
} from "./seed";
import { expect, test } from "./test-fixtures";

// The resolver avatar on a resolved issue distinguishes an operator-attributed
// close from one with no operator attached. The latter is *not* phrased as an
// actor ("automation") — it says the healthcheck recovered, which is what
// canopy actually observed and doesn't claim nobody intervened.
test.describe("resolver attribution", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("an unattributed resolve reads as the healthcheck recovering", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Recovery Group" });
		const server = await seedServer(sql, {
			name: "recovery-srv",
			groupId: group.id,
		});
		await seedIssue(sql, {
			serverId: server.id,
			message: "auto-cleared issue",
			resolved: true,
			resolvedBy: null,
		});

		// showAll=1 turns off the active-only filter so resolved issues show.
		await page.goto("/incidents?showAll=1");
		await expect(page.getByText("auto-cleared issue")).toBeVisible();

		// The resolver avatar is the only avatar on the page (no incidents
		// seeded). Hover it to reveal the MUI tooltip.
		await page.locator(".MuiAvatar-root").first().hover();
		const tooltip = page.getByRole("tooltip");
		await expect(tooltip).toContainText("the healthcheck recovering");
		await expect(tooltip).not.toContainText("automation");
	});

	test("an operator-attributed resolve names the operator", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Operator Group" });
		const server = await seedServer(sql, {
			name: "operator-srv",
			groupId: group.id,
		});
		await seedTailscaleUser(sql, {
			login: "ada@example.test",
			name: "Ada Lovelace",
		});
		await seedIssue(sql, {
			serverId: server.id,
			message: "hand-resolved issue",
			resolved: true,
			resolvedBy: "ada@example.test",
			resolvedReason: "fixed",
		});

		await page.goto("/incidents?showAll=1");
		await expect(page.getByText("hand-resolved issue")).toBeVisible();

		await page.locator(".MuiAvatar-root").first().hover();
		const tooltip = page.getByRole("tooltip");
		await expect(tooltip).toContainText("Ada Lovelace");
		await expect(tooltip).not.toContainText("the healthcheck recovering");
	});
});
