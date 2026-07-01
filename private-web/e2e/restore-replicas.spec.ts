import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedDevice,
	seedRestoreCheck,
	seedRestoreConsumerCapability,
	seedRestoreReplica,
	seedServer,
	seedServerGroup,
	seedServerGroupBackupConfig,
	type Sql,
} from "./seed";

// The e2e fixture runs the private-server in a debug build, so the Tailscale
// auth bypass treats every caller as `admin@localhost` (an admin).
//
// The restore-replica UI lives inside each group's backup page
// (`/groups/:id/backups`, shown once the group has a ready backup config); the
// fleet-wide consumer roster lives in Settings.

test.describe("restore replicas", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	/** A group with a ready backup config, so its backup page renders the panels
	 * (including the restore-replicas section). */
	async function groupWithBackups(
		sql: Sql,
		name: string,
	): Promise<string> {
		const group = await seedServerGroup(sql, { name });
		await seedServerGroupBackupConfig(sql, { groupId: group.id, status: "ready" });
		return group.id;
	}

	test("empty state shows the no-declarations banner", async ({ page, sql }) => {
		const groupId = await groupWithBackups(sql, "empty-group");
		await page.goto(`/groups/${groupId}/backups`);
		await expect(
			page.getByText(/no restore replicas declared for this group/i),
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
		const groupId = await groupWithBackups(sql, "rr-group");

		await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			intent: "verify",
			name: "verify-all",
		});
		await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			intent: "analytics",
			name: "analytics-all",
		});

		await page.goto(`/groups/${groupId}/backups`);

		const verifyRow = page.getByRole("row", { name: /verify-all/ });
		const analyticsRow = page.getByRole("row", { name: /analytics-all/ });
		await expect(verifyRow).toBeVisible();
		await expect(analyticsRow).toBeVisible();
		await expect(analyticsRow.getByText("gap")).toBeVisible();
		await expect(verifyRow.getByText("gap")).toHaveCount(0);
	});

	test("settings lists restore consumers and their capabilities", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify", "disaster-recovery"],
		});

		await page.goto("/settings/restore-consumers");
		await expect(page.getByText("verify").first()).toBeVisible();
		await expect(page.getByText("disaster-recovery").first()).toBeVisible();
	});

	test("deleting a declaration removes it", async ({ page, sql }) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const groupId = await groupWithBackups(sql, "del-group");
		await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			intent: "verify",
			name: "doomed",
		});

		await page.goto(`/groups/${groupId}/backups`);
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
		const groupId = await groupWithBackups(sql, "tog-group");
		const replica = await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			intent: "verify",
			name: "togglable",
			enabled: true,
		});

		await page.goto(`/groups/${groupId}/backups`);
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
		const groupId = await groupWithBackups(sql, "chk-group");
		const server = await seedServer(sql, { groupId, name: "chk-srv" });
		await seedRestoreCheck(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			serverId: server.id,
			outcome: "failure",
			replicaHealthy: false,
			error: "restore blew up",
		});

		await page.goto(`/groups/${groupId}/backups`);
		await expect(page.getByText(/recent restore checks/i)).toBeVisible();
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
		const groupId = await groupWithBackups(sql, "create-group");
		await seedServer(sql, { groupId, name: "srv-a" });

		await page.goto(`/groups/${groupId}/backups`);
		await page.getByRole("button", { name: /declare replica/i }).click();

		const dialog = page.getByRole("dialog");
		await dialog.getByLabel("Consumer").click();
		await page.getByRole("option").first().click();
		await dialog.getByLabel("Name").fill("dialog-made");
		await dialog.getByRole("button", { name: /^declare$/i }).click();

		await expect(page.getByRole("row", { name: /dialog-made/ })).toBeVisible();
		const rows = await sql.query<{ name: string }>(
			"SELECT name FROM restore_replicas WHERE name = 'dialog-made'",
		);
		expect(rows).toHaveLength(1);
	});
});
