import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedDevice,
	seedRestoreCheck,
	seedRestoreConsumerCapability,
	seedRestoreReplica,
	seedServer,
	seedServerGroup,
} from "./seed";

// The e2e fixture runs the private-server in a debug build, so the Tailscale
// auth bypass treats every caller as `admin@localhost` (an admin). These specs
// exercise the operator-facing managed-restore UI.

test.describe("restore replicas", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("empty state shows the no-declarations banner", async ({ page }) => {
		await page.goto("/restore-replicas");
		await expect(
			page.getByText(/no restore replicas declared/i),
		).toBeVisible();
	});

	test("a seeded declaration renders; an unsupported intent is flagged as a gap", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const group = await seedServerGroup(sql, { name: "rr-group" });

		// Supported intent — no gap.
		await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId: group.id,
			intent: "verify",
			name: "verify-all",
		});
		// Unsupported intent — gap.
		await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId: group.id,
			intent: "analytics",
			name: "analytics-all",
		});

		await page.goto("/restore-replicas");

		const verifyRow = page.getByRole("row", { name: /verify-all/ });
		const analyticsRow = page.getByRole("row", { name: /analytics-all/ });
		await expect(verifyRow).toBeVisible();
		await expect(analyticsRow).toBeVisible();
		// The unsupported declaration carries a gap chip; the supported one does not.
		await expect(analyticsRow.getByText("gap")).toBeVisible();
		await expect(verifyRow.getByText("gap")).toHaveCount(0);
	});

	test("consumers panel lists the device and its capabilities", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify", "disaster-recovery"],
		});

		await page.goto("/restore-replicas");
		// The consumer's intents render as chips.
		await expect(page.getByText("verify").first()).toBeVisible();
		await expect(page.getByText("disaster-recovery").first()).toBeVisible();
	});

	test("deleting a declaration removes it", async ({ page, sql }) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const group = await seedServerGroup(sql, { name: "del-group" });
		await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId: group.id,
			intent: "verify",
			name: "doomed",
		});

		await page.goto("/restore-replicas");
		await expect(page.getByRole("row", { name: /doomed/ })).toBeVisible();
		await page.getByRole("button", { name: "delete doomed" }).click();
		await expect(page.getByRole("row", { name: /doomed/ })).toHaveCount(0);

		const rows = await sql.query<{ count: string }>(
			"SELECT count(*) AS count FROM restore_replicas",
		);
		expect(Number(rows[0]!.count)).toBe(0);
	});

	test("toggling enabled flips the row in the database", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const group = await seedServerGroup(sql, { name: "tog-group" });
		const replica = await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId: group.id,
			intent: "verify",
			name: "togglable",
			enabled: true,
		});

		await page.goto("/restore-replicas");
		await page
			.getByRole("row", { name: /togglable/ })
			.locator('input[type="checkbox"]')
			.click();

		await expect
			.poll(async () => {
				const rows = await sql.query<{ enabled: boolean }>(
					"SELECT enabled FROM restore_replicas WHERE id = $1",
					[replica.id],
				);
				return rows[0]?.enabled;
			})
			.toBe(false);
	});

	test("recent restore checks render with their outcome", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		const group = await seedServerGroup(sql, { name: "chk-group" });
		const server = await seedServer(sql, { groupId: group.id, name: "chk-srv" });
		await seedRestoreCheck(sql, {
			consumerDeviceId: consumer.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "failure",
			replicaHealthy: false,
			error: "restore blew up",
		});

		await page.goto("/restore-replicas");
		const checksHeading = page.getByRole("heading", {
			name: /recent restore checks/i,
		});
		await expect(checksHeading).toBeVisible();
		// The failed check renders with a "failed" chip.
		await expect(page.getByText("failed")).toBeVisible();
	});

	test("declaring a replica through the dialog persists it", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const group = await seedServerGroup(sql, { name: "create-group" });
		await seedServer(sql, { groupId: group.id, name: "srv-a" });

		await page.goto("/restore-replicas");
		await page.getByRole("button", { name: /declare replica/i }).click();

		await page.getByLabel("Consumer").click();
		await page.getByRole("option").first().click();
		await page.getByLabel("Group").click();
		await page.getByRole("option", { name: "create-group" }).click();
		await page.getByLabel("Name").fill("dialog-made");
		await page
			.getByRole("button", { name: /^declare$/i })
			.click();

		await expect(
			page.getByRole("row", { name: /dialog-made/ }),
		).toBeVisible();
		const rows = await sql.query<{ name: string }>(
			"SELECT name FROM restore_replicas WHERE name = 'dialog-made'",
		);
		expect(rows).toHaveLength(1);
	});
});
