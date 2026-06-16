import {
	resetSeededTables,
	seedIssue,
	seedServerGroup,
} from "./seed";
import { expect, test } from "./test-fixtures";

// Group-scoped issues (server_id NULL, server_group_id set) must not render a
// per-server link (`/servers/null`) or a per-server silence action — both only
// make sense for server-scoped issues.
test.describe("group-scoped issue rendering", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("a group-scoped issue has no /servers/null link", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "GroupScopeRender" });
		await seedIssue(sql, {
			serverGroupId: group.id,
			source: "backup",
			ref: "stale",
			message: "group-wide backup issue",
		});

		await page.goto("/incidents?showAll=1");
		await expect(page.getByText("group-wide backup issue")).toBeVisible();

		// No broken server link is rendered for the group-scoped issue.
		await expect(page.locator('a[href="/servers/null"]')).toHaveCount(0);
	});

	test("a group-scoped issue offers no per-server silence button", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "GroupScopeSilence" });
		await seedIssue(sql, {
			serverGroupId: group.id,
			source: "backup",
			ref: "stale",
			message: "group-wide silence target",
		});

		await page.goto("/incidents?showAll=1");
		await expect(page.getByText("group-wide silence target")).toBeVisible();

		// Expand the row to reveal its action buttons (admin in the e2e auth
		// bypass), then open the silence panel.
		await page.getByRole("button", { name: /^expand$/i }).first().click();
		await page.getByRole("button", { name: /silence ref/i }).click();

		// The per-server silence action must not be present for a group-scoped
		// issue; only the group-scope action (if any) is valid.
		await expect(
			page.getByRole("button", { name: /for this server/i }),
		).toHaveCount(0);
	});
});
