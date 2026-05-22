import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedServer, seedServerGroup } from "./seed";

test.describe("group detail page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the group's name, notes, tags and member servers", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, {
			name: "watched-cluster",
			notes: "ops handover note",
			tags: { env: "prod", tier: "1" },
		});
		const memberA = await seedServer(sql, {
			name: "member-a",
			groupId: group.id,
		});
		const memberB = await seedServer(sql, {
			name: "member-b",
			groupId: group.id,
		});

		await page.goto(`/groups/${group.id}`);

		await expect(
			page.getByRole("heading", { name: group.name, level: 1 }),
		).toBeVisible();
		await expect(page.getByText("ops handover note")).toBeVisible();
		await expect(page.getByText("env=prod")).toBeVisible();
		await expect(page.getByText("tier=1")).toBeVisible();
		await expect(
			page.getByRole("link", { name: new RegExp(memberA.name) }),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: new RegExp(memberB.name) }),
		).toBeVisible();
	});

	test("an empty group lists no servers", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "lonely-group" });
		await page.goto(`/groups/${group.id}`);
		await expect(
			page.getByRole("heading", { name: group.name, level: 1 }),
		).toBeVisible();
		await expect(page.getByText(/no servers in this group/i)).toBeVisible();
	});
});

test.describe("group edit page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("pre-fills the form with the group's name, notes and tags", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, {
			name: "editable-group",
			notes: "be careful",
			tags: { region: "au" },
		});

		await page.goto(`/groups/${group.id}/edit`);

		await expect(page.getByLabel(/^Name$/i)).toHaveValue(group.name);
		await expect(page.getByLabel(/^Notes$/i)).toHaveValue("be careful");
		await expect(page.getByLabel(/^Key$/i)).toHaveValue("region");
		await expect(page.getByLabel(/^Value$/i)).toHaveValue("au");
	});
});
