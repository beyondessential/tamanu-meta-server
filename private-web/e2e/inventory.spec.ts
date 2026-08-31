import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedServer, seedServerGroup } from "./seed";

// The inventory a configuration run would receive, presented on the group
// page: one panel per environment, the group's variables once, and each
// server's own variables under it.
// spec: INV#presentation
test.describe("group inventory", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("shows one panel per environment, with each server's address", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, {
			name: "kamaka",
			tags: { timezone: "Pacific/Auckland", tamanu_version: "v2.54.8" },
		});
		await seedServer(sql, {
			name: "kamaka-prod-central",
			host: "https://central.kamaka.e2e.invalid",
			kind: "central",
			rank: "production",
			groupId: group.id,
			tags: { ansible_user: "ubuntu" },
		});
		await seedServer(sql, {
			name: "kamaka-demo-central",
			host: "https://central.demo.kamaka.e2e.invalid",
			kind: "central",
			rank: "demo",
			groupId: group.id,
			tags: {},
		});

		await page.goto(`/groups/${group.id}`);

		const inventory = page
			.locator("div")
			.filter({ has: page.getByRole("heading", { name: "Inventory" }) })
			.last();
		await expect(inventory.getByText("production")).toBeVisible();
		await expect(inventory.getByText("demo")).toBeVisible();

		// The group's values are shown once per environment rather than repeated
		// under every server.
		await expect(
			inventory.getByText("timezone = Pacific/Auckland").first(),
		).toBeVisible();
		await expect(
			inventory.getByText("ansible_user = ubuntu"),
		).toBeVisible();

		// Address falls back to the recorded host where no device is bound.
		await expect(inventory.getByText("central.kamaka.e2e.invalid")).toBeVisible();
		await expect(
			inventory.getByText("kamaka-prod-central", { exact: true }),
		).toBeVisible();
		await expect(
			inventory.getByText("kamaka-demo-central", { exact: true }),
		).toBeVisible();
	});

	test("marks a server that overrides a group value", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, {
			name: "drifting",
			tags: { elastic_agent_enabled: "false" },
		});
		await seedServer(sql, {
			name: "drifting-prod-central",
			kind: "central",
			rank: "production",
			groupId: group.id,
			tags: { elastic_agent_enabled: "true" },
		});
		await seedServer(sql, {
			name: "drifting-prod-facility",
			kind: "facility",
			rank: "production",
			groupId: group.id,
			tags: {},
		});

		await page.goto(`/groups/${group.id}`);

		// The group's value and the server's override are both present, and the
		// override carries the tooltip saying which it is.
		await expect(
			page.getByText("elastic_agent_enabled = false"),
		).toBeVisible();
		const override = page.getByText("elastic_agent_enabled = true");
		await expect(override).toBeVisible();
		await override.hover();
		await expect(page.getByText("Overrides the group's value")).toBeVisible();

		// A server setting nothing of its own says so, rather than showing the
		// group's values again as if it set them.
		await expect(page.getByText("Sets nothing of its own")).toBeVisible();
	});
});
