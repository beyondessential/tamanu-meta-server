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

		const inventory = page.getByTestId("group-inventory");
		const production = inventory.getByTestId("environment-production");
		const demo = inventory.getByTestId("environment-demo");
		await expect(production).toBeVisible();
		await expect(demo).toBeVisible();

		// The group's values are shown once per environment rather than repeated
		// under every server.
		await expect(
			production.getByText("timezone = Pacific/Auckland").first(),
		).toBeVisible();
		await expect(production.getByText("ansible_user = ubuntu")).toBeVisible();

		// Each environment carries only its own servers, and the address falls
		// back to the recorded host where no device is bound.
		await expect(production.getByTestId("inventory-server")).toHaveCount(1);
		await expect(
			production.getByText("kamaka-prod-central", { exact: true }),
		).toBeVisible();
		await expect(
			production.getByText("central.kamaka.e2e.invalid", { exact: true }),
		).toBeVisible();
		await expect(demo.getByTestId("inventory-server")).toHaveCount(1);
		await expect(
			demo.getByText("kamaka-demo-central", { exact: true }),
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

		const production = page
			.getByTestId("group-inventory")
			.getByTestId("environment-production");

		// The group's value and the server's override are both shown, and only
		// the override is marked as one.
		await expect(
			production.getByText("elastic_agent_enabled = false"),
		).toBeVisible();
		await expect(production.getByTestId("overriding-var")).toHaveText(
			"elastic_agent_enabled = true",
		);

		// A server setting nothing of its own says so, rather than showing the
		// group's values again as if it set them.
		await expect(production.getByText("Sets nothing of its own")).toBeVisible();
	});

	test("shows a secret variable by name and never by value", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "kamaka", tags: {} });
		await seedServer(sql, {
			name: "kamaka-prod-central",
			kind: "central",
			rank: "production",
			groupId: group.id,
			tags: {},
		});

		await page.goto(`/groups/${group.id}`);
		const production = page
			.getByTestId("group-inventory")
			.getByTestId("environment-production");

		const form = production.getByTestId("set-secret");
		await form.getByLabel("Name").fill("salt");
		await form.getByLabel("Value").fill("pepper");
		await form.getByRole("button", { name: "Set" }).click();

		await expect(production.getByTestId("secret-var")).toContainText(
			"salt = secret",
		);
		await expect(page.getByText("pepper")).toHaveCount(0);

		// And it is a variable, not a tag: the tag of that name is refused.
		await form.getByLabel("Name").fill("salt");
		await form.getByLabel("Value").fill("again");
		await form.getByRole("button", { name: "Set" }).click();
		await expect(production.getByTestId("secret-var")).toHaveCount(1);

		await production.getByTestId("remove-salt").click();
		await expect(production.getByTestId("secret-var")).toHaveCount(0);
	});
});
