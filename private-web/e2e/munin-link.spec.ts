import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedDevice, seedServer, seedStatus } from "./seed";

// spec: SVC#munin-link
test.describe("munin link on server detail", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("links to the tailnet host on :4950 when the server reports munin", async ({
		page,
		sql,
	}) => {
		const device = await seedDevice(sql, {
			tailscaleNodeName: "munin-box.e2e.ts.net",
		});
		const server = await seedServer(sql, {
			name: "has-munin",
			deviceId: device.id,
		});
		await seedStatus(sql, { serverId: server.id, extra: { munin: true } });

		await page.goto(`/servers/${server.id}`);

		const munin = page.getByRole("link", { name: "Munin" });
		await expect(munin).toBeVisible();
		await expect(munin).toHaveAttribute(
			"href",
			"https://munin-box.e2e.ts.net:4950/",
		);
		await expect(munin).toHaveAttribute("target", "_blank");
	});

	test("offers no Munin link when the server never reported munin", async ({
		page,
		sql,
	}) => {
		const device = await seedDevice(sql, {
			tailscaleNodeName: "plain-box.e2e.ts.net",
		});
		const server = await seedServer(sql, {
			name: "no-munin",
			deviceId: device.id,
		});
		await seedStatus(sql, { serverId: server.id, extra: { uptimeSecs: 3600 } });

		await page.goto(`/servers/${server.id}`);

		// Wait for the page to render before asserting the link's absence.
		await expect(
			page.getByRole("heading", { level: 1, name: /no-munin/ }),
		).toBeVisible();
		await expect(page.getByRole("link", { name: "Munin" })).toHaveCount(0);
	});
});
