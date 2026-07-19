import {
	resetSeededTables,
	seedCheckPolicy,
	seedIssue,
	seedServer,
	seedServerGroup,
} from "./seed";
import { expect, test } from "./test-fixtures";

// Group-scoped issues (server_id NULL, server_group_id set) must not render a
// per-server link (`/servers/null`) or a per-server silence action — both only
// make sense for server-scoped issues.
test.describe("issue check documentation", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("the ? button on an issue row pops up the check's documentation", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "doc-issue-server" });
		await seedIssue(sql, {
			serverId: server.id,
			ref: "health/postgres",
			message: "postgres is down",
		});
		await seedCheckPolicy(sql, {
			checkName: "postgres",
			documentation:
				"## Description\n\nWatches the PostgreSQL connection.\n\n## Solve\n\nCheck pg_hba.conf.",
		});

		await page.goto("/incidents?showAll=1");
		await expect(page.getByText("postgres is down")).toBeVisible();
		await page
			.getByRole("button", { name: "Documentation for postgres" })
			.click();
		await expect(
			page.getByText("Watches the PostgreSQL connection."),
		).toBeVisible();
	});
});

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

// The manual-condition form speaks the result vocabulary: an operator
// raises a failed (optionally escalating) or warning condition, which
// files under the manual source and grades through the catalog.
test.describe("manual conditions", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("raising a failed condition from the server page files an issue", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "ManualGroup" });
		const server = await seedServer(sql, {
			name: "manual-target",
			kind: "central",
			groupId: group.id,
		});

		await page.goto(`/servers/${server.id}`);
		await page.getByRole("button", { name: /new incident/i }).click();

		const dialog = page.getByRole("dialog");
		await expect(dialog.getByLabel("Result")).toBeVisible();
		// A failed condition offers the immediate-notify (escalates) toggle.
		await expect(
			dialog.getByRole("switch", { name: /notify immediately/i }),
		).toBeVisible();
		await dialog
			.getByLabel("Description (short)")
			.fill("Manually raised trouble");
		await dialog.getByLabel("Message").fill("operator saw smoke");
		await dialog.getByRole("button", { name: /^submit$/i }).click();
		await expect(dialog).not.toBeVisible();

		// The condition filed as a failure: it appears in the fleet issue
		// list (the never-checked-in server page shows setup instructions,
		// not an issues panel).
		await page.goto("/incidents?showAll=1");
		await expect(page.getByText("Manually raised trouble")).toBeVisible();
	});
});
