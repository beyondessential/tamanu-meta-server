import {
	resetSeededTables,
	seedIssue,
	seedServer,
	seedServerGroup,
} from "./seed";
import { expect, test } from "./test-fixtures";

// The server-detail header button into the incidents view. Its "active
// issues?" question is answered by a group-scoped query, so it needs the
// server's *group* id — feeding it the server id matches no group at all and
// the button reports a clean server no matter what is wrong with it.
test.describe("server detail incidents link", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("reports active issues and links to the group's incidents", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "link-group" });
		const server = await seedServer(sql, {
			name: "link-server",
			groupId: group.id,
		});
		// An active issue but no incident — the state that distinguishes
		// "Active issues" from "Past issues".
		await seedIssue(sql, {
			serverId: server.id,
			ref: "health/postgres",
			message: "postgres is down",
		});

		await page.goto(`/fleet/applications/${server.id}`);
		const link = page.getByRole("link", { name: "Active issues" });
		await expect(link).toBeVisible();
		await expect(link).toHaveAttribute(
			"href",
			`/incidents?group=${group.id}`,
		);

		// The linked page filters to a real group, so the issue is there.
		await link.click();
		await expect(page.getByText("postgres is down")).toBeVisible();
	});

	test("reports past issues when the group has nothing active", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "quiet-group" });
		const server = await seedServer(sql, {
			name: "quiet-server",
			groupId: group.id,
		});

		await page.goto(`/fleet/applications/${server.id}`);
		const link = page.getByRole("link", { name: "Past issues" });
		await expect(link).toBeVisible();
		await expect(link).toHaveAttribute(
			"href",
			`/incidents?group=${group.id}&showAll=1`,
		);
	});

	test("an ungrouped server links to the unfiltered incidents view", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, {
			name: "ungrouped-server",
			groupId: null,
		});

		await page.goto(`/fleet/applications/${server.id}`);
		// No group to filter by, so no group query parameter — and certainly
		// not the server id standing in for one.
		const link = page.getByRole("link", { name: "Past issues" });
		await expect(link).toBeVisible();
		await expect(link).toHaveAttribute("href", "/incidents?showAll=1");
	});
});
