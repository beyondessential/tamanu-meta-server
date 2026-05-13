import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedDevice, seedDeviceKey } from "./seed";

test.describe("devices index", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the three tabs and the search input", async ({ page }) => {
		await page.goto("/devices");
		await expect(page.getByRole("tab", { name: "Search" })).toBeVisible();
		await expect(
			page.getByRole("tab", { name: "Untrusted devices" }),
		).toBeVisible();
		await expect(
			page.getByRole("tab", { name: "Trusted devices", exact: true }),
		).toBeVisible();
		await expect(
			page.getByRole("searchbox", { name: /Search by public key/i }),
		).toBeVisible();
	});
});

test.describe("devices list tabs", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("untrusted tab shows untrusted devices, trusted tab shows trusted ones", async ({
		page,
		sql,
	}) => {
		const untrusted = await seedDevice(sql, { role: "untrusted" });
		await seedDeviceKey(sql, {
			deviceId: untrusted.id,
			name: "naughty-cert",
			isActive: true,
		});
		const trusted = await seedDevice(sql, { role: "server" });
		await seedDeviceKey(sql, {
			deviceId: trusted.id,
			name: "blessed-cert",
			isActive: true,
		});

		// Untrusted tab.
		await page.goto("/devices/untrusted");
		await expect(page).toHaveURL(/\/devices\/untrusted$/);
		await expect(page.getByText("naughty-cert")).toBeVisible();
		await expect(page.getByText("blessed-cert")).not.toBeVisible();

		// Trusted tab.
		await page.goto("/devices/trusted");
		await expect(page).toHaveURL(/\/devices\/trusted$/);
		await expect(page.getByText("blessed-cert")).toBeVisible();
		await expect(page.getByText("naughty-cert")).not.toBeVisible();
	});
});

test.describe("device detail page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders the role for a seeded device", async ({ page, sql }) => {
		const device = await seedDevice(sql, { role: "server" });
		await seedDeviceKey(sql, {
			deviceId: device.id,
			name: "detail-cert",
			isActive: true,
		});

		await page.goto(`/devices/${device.id}`);

		// The named key shows up as the device name in the header.
		await expect(
			page.getByRole("heading", { name: "detail-cert", level: 1 }),
		).toBeVisible();
	});
});
