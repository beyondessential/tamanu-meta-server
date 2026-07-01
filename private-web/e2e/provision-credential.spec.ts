import { readFile } from "node:fs/promises";
import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedDevice, seedDeviceKey } from "./seed";

test.describe("provision device credentials", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("create device mints a downloadable age-encrypted key", async ({
		page,
	}) => {
		await page.goto("/devices/all");
		await page.getByRole("button", { name: "Create device" }).click();

		// Name the key so we can find the device in the list afterwards.
		await page
			.getByLabel("Key name (optional)")
			.fill("e2e-provisioned");
		await page.getByRole("button", { name: "Provision", exact: true }).click();

		// One-shot result view: the "shown once" warning and a passphrase.
		await expect(page.getByText(/shown once/i)).toBeVisible();
		await expect(
			page.getByRole("button", { name: "Download key file" }),
		).toBeVisible();

		// The download is a standard age file.
		const downloadPromise = page.waitForEvent("download");
		await page.getByRole("button", { name: "Download key file" }).click();
		const download = await downloadPromise;
		expect(download.suggestedFilename()).toMatch(/\.pem\.age$/);
		const path = await download.path();
		const bytes = await readFile(path);
		expect(bytes.subarray(0, 21).toString("ascii")).toBe(
			"age-encryption.org/v1",
		);

		// Closing returns to the list, which now shows the new device by its key.
		await page.getByRole("button", { name: /Done|Close/ }).click();
		await expect(page.getByText("e2e-provisioned")).toBeVisible();
	});

	test("provision credential on an existing device adds a key", async ({
		page,
		sql,
	}) => {
		const device = await seedDevice(sql, { role: "releaser" });
		await seedDeviceKey(sql, {
			deviceId: device.id,
			name: "original-key",
			isActive: true,
		});

		await page.goto(`/devices/${device.id}`);
		await page
			.getByRole("button", { name: "Provision credential" })
			.click();
		await page.getByRole("button", { name: "Provision", exact: true }).click();

		const downloadPromise = page.waitForEvent("download");
		await page.getByRole("button", { name: "Download key file" }).click();
		const download = await downloadPromise;
		expect(download.suggestedFilename()).toMatch(/^canopy-releaser-.*\.pem\.age$/);

		await page.getByRole("button", { name: /Done|Close/ }).click();

		// The device now has two active keys.
		await expect(page.getByText("Public Keys (2)")).toBeVisible();
	});
});
