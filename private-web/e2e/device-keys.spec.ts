import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedDevice, seedDeviceKey } from "./seed";

// A throwaway P-256 public key (SubjectPublicKeyInfo PEM) for the add-from-key flow.
const PUBLIC_KEY_PEM = `-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEv6HzSyPiyx1o98LE8fiIkFQ5u5pv
dvabF1OOXhPzbKHPpoRAQsJQ3XecL2ONQ1NBco65X7QT82vr7m13i156jQ==
-----END PUBLIC KEY-----`;

test.describe("device key management", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("disable then re-enable a key", async ({ page, sql }) => {
		const device = await seedDevice(sql, { role: "machine" });
		await seedDeviceKey(sql, {
			deviceId: device.id,
			name: "rotating-key",
			isActive: true,
		});

		await page.goto(`/devices/${device.id}`);

		// Disable the key — it stays listed, marked disabled, with an Enable action.
		await page.getByRole("button", { name: "Disable", exact: true }).click();
		await expect(page.getByText("disabled")).toBeVisible();
		await expect(
			page.getByRole("button", { name: "Enable", exact: true }),
		).toBeVisible();

		// Re-enable it.
		await page.getByRole("button", { name: "Enable", exact: true }).click();
		await expect(
			page.getByRole("button", { name: "Disable", exact: true }),
		).toBeVisible();
	});

	test("add a key from a pasted public key", async ({ page, sql }) => {
		const device = await seedDevice(sql, { role: "machine" });
		await seedDeviceKey(sql, {
			deviceId: device.id,
			name: "existing-key",
			isActive: true,
		});

		await page.goto(`/devices/${device.id}`);
		await expect(page.getByText("Public Keys (1)")).toBeVisible();

		await page.getByRole("button", { name: "Add from public key" }).click();
		await page.getByLabel("Public key (PEM)").fill(PUBLIC_KEY_PEM);
		await page.getByLabel("Key name (optional)").fill("pasted-key");
		await page.getByRole("button", { name: "Add key", exact: true }).click();

		await expect(page.getByText("Public Keys (2)")).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "pasted-key", level: 6 }),
		).toBeVisible();
	});
});
