import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedBackupRepoStats,
	seedBackupRun,
	seedDevice,
	seedServer,
	seedServerBackupCapability,
	seedServerGroup,
	seedServerGroupBackupConfig,
} from "./seed";

// The e2e fixture runs the private-server in a debug build, so the Tailscale
// auth bypass treats every caller as `admin@localhost` (an admin). These specs
// therefore exercise the admin-facing flows; non-admin gating is covered by
// the Rust security() annotations + the prod auth path.

test.describe("backups zero-state + config", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("unconfigured group shows the set-up CTA and panel zero-state", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "no-backups" });

		await page.goto(`/groups/${group.id}`);
		// The CTA is a MUI Button rendered as a router link (role "link").
		await expect(
			page.getByRole("link", { name: /set up backups/i }),
		).toBeVisible();

		await page.goto(`/groups/${group.id}/backups`);
		await expect(page.getByText(/backups not set up/i)).toBeVisible();
	});

	test("wizard create (empty bucket) writes a from-birth config row", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "cfg-group" });

		await page.goto(`/groups/${group.id}/backups/config`);
		await page.getByLabel("Bucket").fill("bes-kopia-created");
		await page
			.getByLabel("Target role ARN")
			.fill("arn:aws:iam::999:role/created");
		await page
			.getByLabel("Maintenance role ARN")
			.fill("arn:aws:iam::999:role/created-maint");
		await page.getByRole("button", { name: /check bucket/i }).click();
		await expect(page.getByText(/empty bucket/i)).toBeVisible();
		// No schedule step — schedule/retention inherit the per-type defaults.
		await page.getByRole("button", { name: /create & provision/i }).click();

		await expect(page).toHaveURL(new RegExp(`/groups/${group.id}/backups$`));

		const rows = await sql.query<{
			status: string;
			mode: string;
			maintenance_role_arn: string;
		}>(
			`SELECT status, mode, maintenance_role_arn
			 FROM server_group_backup_config WHERE group_id = $1`,
			[group.id],
		);
		expect(rows).toHaveLength(1);
		expect(rows[0]!.status).toBe("provisioning");
		expect(rows[0]!.mode).toBe("from_birth");
		expect(rows[0]!.maintenance_role_arn).toBe(
			"arn:aws:iam::999:role/created-maint",
		);
		// The wizard writes no per-(group,type) schedule override.
		const sched = await sql.query(
			`SELECT 1 FROM server_group_backup_schedule WHERE group_id = $1`,
			[group.id],
		);
		expect(sched).toHaveLength(0);
	});

	test("wizard create (existing repo) requires a passphrase and persists passphrase mode", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "passphrase-group" });
		await page.goto(`/groups/${group.id}/backups/config`);
		// `…existing…` → the fake prober reports an existing kopia repo.
		await page.getByLabel("Bucket").fill("bes-existing-repo");
		await page.getByLabel("Target role ARN").fill("arn:aws:iam::999:role/dev");
		await page
			.getByLabel("Maintenance role ARN")
			.fill("arn:aws:iam::999:role/maint");
		await page.getByRole("button", { name: /check bucket/i }).click();

		await expect(page.getByText(/existing kopia repository/i)).toBeVisible();
		// Provisioning is gated on the passphrase.
		await expect(
			page.getByRole("button", { name: /create & provision/i }),
		).toBeDisabled();
		await page
			.getByLabel("Existing repository passphrase")
			.fill("an-existing-repo-passphrase");
		await page.getByRole("button", { name: /create & provision/i }).click();

		await expect(page).toHaveURL(new RegExp(`/groups/${group.id}/backups$`));
		const rows = await sql.query<{ mode: string }>(
			`SELECT mode FROM server_group_backup_config WHERE group_id = $1`,
			[group.id],
		);
		expect(rows[0]!.mode).toBe("passphrase");
	});

	test("wizard blocks other (non-kopia) content", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "other-group" });
		await page.goto(`/groups/${group.id}/backups/config`);
		// `…other…` → the fake prober reports non-kopia content.
		await page.getByLabel("Bucket").fill("bes-other-stuff");
		await page.getByLabel("Target role ARN").fill("arn");
		await page.getByLabel("Maintenance role ARN").fill("maint-arn");
		await page.getByRole("button", { name: /check bucket/i }).click();

		await expect(page.getByText(/other \(non-kopia\) content/i)).toBeVisible();
		// No proceeding: Create & provision disabled, Re-check offered.
		await expect(
			page.getByRole("button", { name: /create & provision/i }),
		).toBeDisabled();
		await expect(page.getByRole("button", { name: /re-check/i })).toBeVisible();
	});

	test("'shared backups' option provisions a canopy-managed bucket (no AWS account)", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Acme Prod" });

		await page.goto(`/groups/${group.id}/backups/config`);
		// Pick shared-account backups — no bucket/roles to enter, no probe step.
		await page.getByRole("button", { name: /shared backups/i }).click();
		await page.getByRole("button", { name: /create & provision/i }).click();

		await expect(page).toHaveURL(new RegExp(`/groups/${group.id}/backups$`));

		const rows = await sql.query<{
			status: string;
			mode: string;
			placement: string;
			bucket: string;
		}>(
			`SELECT status, mode, placement, bucket
			 FROM server_group_backup_config WHERE group_id = $1`,
			[group.id],
		);
		expect(rows).toHaveLength(1);
		expect(rows[0]!.placement).toBe("shared");
		expect(rows[0]!.mode).toBe("from_birth");
		expect(rows[0]!.status).toBe("provisioning");
		// Auto-named from the group name + a random suffix.
		expect(rows[0]!.bucket.startsWith("bes-canopy-backup-acme-prod-")).toBe(true);
	});
});

test.describe("backups ready: stats + backup-now", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("per-type schedule inherits the default and saves an override", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "sched-group" });
		const server = await seedServer(sql, { groupId: group.id });
		// An enabled tamanu-postgres capability makes the type appear in the panel.
		await seedServerBackupCapability(sql, { serverId: server.id });
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
		});

		await page.goto(`/groups/${group.id}/backups`);

		// No override yet → inherits the seeded canopy-wide default.
		await expect(page.getByText("tamanu-postgres")).toBeVisible();
		await expect(page.getByText("Inherited default")).toBeVisible();

		// Override the interval to 12h.
		await page.getByRole("button", { name: /^override$/i }).click();
		await page.getByLabel("Back up every (hours)").fill("12");
		await page.getByRole("button", { name: /save override/i }).click();

		await expect(page.getByText("Override", { exact: true })).toBeVisible();
		const rows = await sql.query<{ secs: string }>(
			`SELECT EXTRACT(EPOCH FROM expected_interval)::text AS secs
			 FROM server_group_backup_schedule
			 WHERE group_id = $1 AND type = 'tamanu-postgres'`,
			[group.id],
		);
		expect(rows).toHaveLength(1);
		expect(Number(rows[0]!.secs)).toBe(43200);
	});

	test("stats render with unknown bucket bytes and recent runs", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "stats-group" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "stats-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
			intervalSeconds: 3600,
		});
		await seedBackupRepoStats(sql, {
			groupId: group.id,
			snapshotCount: 42,
			sourceCount: 3,
			logicalBytes: 1048576,
			physicalBytes: 524288,
			bucketBytes: null, // renders as "unknown", not hidden
		});
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "success",
			bytesUploaded: 2048,
		});

		await page.goto(`/groups/${group.id}/backups`);
		await expect(page.getByText(/repository stats/i)).toBeVisible();
		await expect(page.getByText("42")).toBeVisible();
		// bucket_bytes NULL → "unknown" shown, per the indicators rule.
		await expect(page.getByText(/bucket bytes:\s*unknown/i)).toBeVisible();
		await expect(page.getByText(/recent runs/i)).toBeVisible();
		await expect(page.getByText("success")).toBeVisible();
	});

	test("backup-now writes a request row; cancel deletes it", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "now-group" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "now-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
			intervalSeconds: 3600,
		});

		await page.goto(`/groups/${group.id}/backups`);
		await page.getByRole("button", { name: /backup now/i }).click();

		await expect(async () => {
			const rows = await sql.query(
				`SELECT 1 FROM backup_requests WHERE server_id = $1 AND purpose = 'backup'`,
				[server.id],
			);
			expect(rows).toHaveLength(1);
		}).toPass();

		await expect(page.getByText(/requested/i)).toBeVisible();

		await page.getByRole("button", { name: /^cancel$/i }).click();
		await expect(async () => {
			const rows = await sql.query(
				`SELECT 1 FROM backup_requests WHERE server_id = $1`,
				[server.id],
			);
			expect(rows).toHaveLength(0);
		}).toPass();
	});

	test("provisioning with init error shows retry", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "failed-group" });
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "provisioning",
			lastInitError: "kopia repository create failed",
		});

		await page.goto(`/groups/${group.id}/backups`);
		await expect(
			page.getByText(/kopia repository create failed/i),
		).toBeVisible();
		await expect(
			page.getByRole("button", { name: /retry repo creation/i }),
		).toBeVisible();
	});

	test("admin can delete (decommission) the config", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "del-group" });
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
		});

		await page.goto(`/groups/${group.id}/backups`);
		// Header delete → confirm dialog → confirm.
		await page.getByRole("button", { name: /^delete$/i }).click();
		const dialog = page.getByRole("dialog");
		await expect(dialog).toBeVisible();
		await dialog.getByRole("button", { name: /^delete$/i }).click();

		// Config is gone → back to the zero-state.
		await expect(page.getByText(/backups not set up/i)).toBeVisible();
		const rows = await sql.query(
			`SELECT 1 FROM server_group_backup_config WHERE group_id = $1`,
			[group.id],
		);
		expect(rows).toHaveLength(0);
	});
});

test.describe("server backup capabilities", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("server with no capabilities shows the empty state", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "caps-empty-group" });
		const server = await seedServer(sql, {
			name: "caps-empty-srv",
			groupId: group.id,
		});

		await page.goto(`/servers/${server.id}`);
		await expect(
			page.getByText(/no backup types registered for this server/i),
		).toBeVisible();
	});

	test("toggling a capability switch flips enabled in the DB", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "caps-group" });
		const server = await seedServer(sql, {
			name: "caps-srv",
			groupId: group.id,
		});
		await seedServerBackupCapability(sql, {
			serverId: server.id,
			type: "tamanu-postgres",
			enabled: false,
		});

		await page.goto(`/servers/${server.id}`);
		const toggle = page.getByRole("switch", {
			name: /enable tamanu-postgres backups/i,
		});
		await expect(toggle).not.toBeChecked();

		await toggle.click();

		await expect(async () => {
			const rows = await sql.query<{ enabled: boolean }>(
				`SELECT enabled FROM server_backup_capabilities
				 WHERE server_id = $1 AND type = 'tamanu-postgres'`,
				[server.id],
			);
			expect(rows[0]!.enabled).toBe(true);
		}).toPass();

		await expect(toggle).toBeChecked();
	});
});
