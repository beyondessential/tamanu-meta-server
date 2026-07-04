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

	test("a restore check shows its postgres version and expandable health details", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		const groupId = await groupWithBackups(sql, "health-group");
		const server = await seedServer(sql, { groupId, name: "health-srv" });
		await seedRestoreCheck(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			serverId: server.id,
			outcome: "success",
			replicaHealthy: true,
			postgresVersion: "16.3",
			healthDetails: { indexes_fixed: true, live_tuples: 4242 },
		});

		await page.goto(`/groups/${groupId}/backups`);
		// The checks table shows the server as a truncated id, not its name, so
		// locate the check row by its own expand button rather than the server.
		const detailsButton = page.getByRole("button", {
			name: /show health details/i,
		});
		const row = page.getByRole("row").filter({ has: detailsButton });
		// The postgres version is now surfaced in the table.
		await expect(row.getByText("16.3")).toBeVisible();

		// Health details are collapsed until expanded, then shown as JSON.
		await expect(page.getByText(/live_tuples/)).toBeHidden();
		await detailsButton.click();
		await expect(page.getByText(/"indexes_fixed": true/)).toBeVisible();
		await expect(page.getByText(/"live_tuples": 4242/)).toBeVisible();
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

	test("the dialog auto-selects the sole consumer and defaults the name", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const groupId = await groupWithBackups(sql, "solo-group");
		await seedServer(sql, { groupId, name: "srv-a" });

		await page.goto(`/groups/${groupId}/backups`);
		await page.getByRole("button", { name: /declare replica/i }).click();
		const dialog = page.getByRole("dialog");

		// Name defaults to the kebab-cased group name (whole-group scope).
		await expect(dialog.getByLabel("Name")).toHaveValue("solo-group");
		// Picking a server folds the server name into the default.
		await dialog.getByLabel("Server").click();
		await page.getByRole("option", { name: "srv-a" }).click();
		await expect(dialog.getByLabel("Name")).toHaveValue("solo-group-srv-a");

		// The consumer was never picked, yet Declare succeeds — the sole consumer
		// was auto-selected.
		await dialog.getByRole("button", { name: /^declare$/i }).click();
		await expect(
			page.getByRole("row", { name: /solo-group-srv-a/ }),
		).toBeVisible();
	});

	test("the intent dropdown offers only intents the consumer registered", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const groupId = await groupWithBackups(sql, "intent-group");

		await page.goto(`/groups/${groupId}/backups`);
		await page.getByRole("button", { name: /declare replica/i }).click();
		const dialog = page.getByRole("dialog");

		await dialog.getByLabel("Intent").click();
		await expect(page.getByRole("option", { name: "verify" })).toBeVisible();
		// The formerly-hardcoded well-known intents are gone.
		await expect(page.getByRole("option", { name: "analytics" })).toHaveCount(0);
		await expect(
			page.getByRole("option", { name: "disaster-recovery" }),
		).toHaveCount(0);
	});

	test("the dialog shows the intent description and typed parameter fields, and persists a value", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: [
				{
					intent: "analytics",
					description: "Keeps a queryable replica running.",
					semantics: ["check", "url"],
					params: {
						minimum_uptime: { type: "duration", default: 7200 },
						anonymisation: { type: "boolean", default: true },
					},
				},
			],
		});
		const groupId = await groupWithBackups(sql, "param-group");

		await page.goto(`/groups/${groupId}/backups`);
		await page.getByRole("button", { name: /declare replica/i }).click();
		const dialog = page.getByRole("dialog");

		// The sole consumer is auto-selected and its sole intent chosen, so the
		// description and parameter fields for `analytics` render.
		await expect(dialog.getByText("Keeps a queryable replica running.")).toBeVisible();
		const uptime = dialog.getByLabel("minimum_uptime (seconds)");
		await expect(uptime).toBeVisible();
		await expect(dialog.getByLabel("anonymisation")).toBeVisible();

		await uptime.fill("3600");
		await dialog.getByLabel("Name").fill("with-params");
		await dialog.getByRole("button", { name: /^declare$/i }).click();

		await expect(page.getByRole("row", { name: /with-params/ })).toBeVisible();
		const rows = await sql.query<{ params: { minimum_uptime?: number } }>(
			"SELECT params FROM restore_replicas WHERE name = 'with-params'",
		);
		expect(rows).toHaveLength(1);
		expect(rows[0]!.params.minimum_uptime).toBe(3600);
	});

	test("editing a declaration through the dialog updates name, overdue bound, and enabled", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: ["verify"],
		});
		const groupId = await groupWithBackups(sql, "edit-group");
		const replica = await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			intent: "verify",
			name: "before-edit",
			overdueAfterSeconds: 3600,
			enabled: true,
		});

		await page.goto(`/groups/${groupId}/backups`);
		await page.getByRole("button", { name: "edit before-edit" }).click();

		const dialog = page.getByRole("dialog");
		await expect(dialog.getByLabel("Name")).toHaveValue("before-edit");
		await expect(dialog.getByLabel("Overdue after (hours, optional)")).toHaveValue(
			"1",
		);

		await dialog.getByLabel("Name").fill("after-edit");
		await dialog.getByLabel("Overdue after (hours, optional)").fill("4");
		await dialog.getByLabel("Enabled").click();
		await dialog.getByRole("button", { name: /^save$/i }).click();

		await expect(page.getByRole("row", { name: /after-edit/ })).toBeVisible();
		await expect(page.getByRole("row", { name: /before-edit/ })).toHaveCount(0);

		const rows = await sql.query<{
			name: string;
			overdue_after_secs: string;
			enabled: boolean;
		}>(
			`SELECT name, enabled, EXTRACT(EPOCH FROM overdue_after)::text AS overdue_after_secs
			 FROM restore_replicas WHERE id = $1`,
			[replica.id],
		);
		expect(rows).toHaveLength(1);
		expect(rows[0]!.name).toBe("after-edit");
		expect(rows[0]!.enabled).toBe(false);
		expect(Number(rows[0]!.overdue_after_secs)).toBe(4 * 3600);
	});

	test("editing a declaration's parameters persists the typed values", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		await seedRestoreConsumerCapability(sql, {
			deviceId: consumer.id,
			intents: [
				{
					intent: "analytics",
					description: "Keeps a queryable replica running.",
					semantics: ["check", "url"],
					params: {
						minimum_uptime: { type: "duration", default: 7200 },
						anonymisation: { type: "boolean", default: true },
					},
				},
			],
		});
		const groupId = await groupWithBackups(sql, "edit-param-group");
		await seedRestoreReplica(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			intent: "analytics",
			name: "param-edit",
			params: { minimum_uptime: 3600, anonymisation: true },
		});

		await page.goto(`/groups/${groupId}/backups`);
		await page.getByRole("button", { name: "edit param-edit" }).click();

		const dialog = page.getByRole("dialog");
		const uptime = dialog.getByLabel("minimum_uptime (seconds)");
		await expect(uptime).toHaveValue("3600");
		await uptime.fill("1800");
		await dialog.getByRole("button", { name: /^save$/i }).click();

		await expect(page.getByRole("row", { name: /param-edit/ })).toBeVisible();
		const rows = await sql.query<{ params: { minimum_uptime?: number } }>(
			"SELECT params FROM restore_replicas WHERE name = 'param-edit'",
		);
		expect(rows).toHaveLength(1);
		expect(rows[0]!.params.minimum_uptime).toBe(1800);
	});

	test("a restore check surfaces a replica url as a link", async ({
		page,
		sql,
	}) => {
		const consumer = await seedDevice(sql, { role: "backup-restore" });
		const groupId = await groupWithBackups(sql, "url-group");
		const server = await seedServer(sql, { groupId, name: "url-srv" });
		await seedRestoreCheck(sql, {
			consumerDeviceId: consumer.id,
			groupId,
			serverId: server.id,
			outcome: "success",
			replicaHealthy: true,
			healthDetails: { url: "https://replica.example.test/db" },
		});

		await page.goto(`/groups/${groupId}/backups`);
		const link = page.getByRole("link", { name: /open/i });
		await expect(link).toBeVisible();
		await expect(link).toHaveAttribute("href", "https://replica.example.test/db");
	});
});
