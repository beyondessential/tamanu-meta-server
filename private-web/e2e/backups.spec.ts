import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedBackupCredentialIssuance,
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
		await page.getByRole("button", { name: /use an existing bucket/i }).click();
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
		await page.getByRole("button", { name: /use an existing bucket/i }).click();
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
		await page.getByRole("button", { name: /use an existing bucket/i }).click();
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
		// "Create a bucket" = shared-account backups — no bucket/roles, no probe.
		await page.getByRole("button", { name: /create a bucket/i }).click();
		await page.getByRole("button", { name: /create & provision/i }).click();

		await expect(page).toHaveURL(new RegExp(`/groups/${group.id}/backups$`));
		// The panel shows which placement was used.
		await expect(page.getByText(/shared account/i)).toBeVisible();

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

		// Scope to the schedule panel: the type also appears in the Servers
		// panel (as a declared-type label), so a page-wide text match is
		// ambiguous. (Next-expected lives in the Servers panel now, not here.)
		const schedules = page
			.getByRole("heading", { name: /schedule & retention/i })
			.locator("..");
		// No override yet → inherits the seeded canopy-wide default.
		await expect(schedules.getByText("tamanu-postgres")).toBeVisible();
		await expect(schedules.getByText("Inherited default")).toBeVisible();

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
		// The run carries a server_id, so the table names which server it's from.
		const runs = page.getByRole("table").last();
		await expect(runs.getByText("stats-srv")).toBeVisible();
	});

	test("recent run shows a truncated, copyable snapshot id", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "snap-group" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "snap-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
			intervalSeconds: 3600,
		});
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "success",
			bytesUploaded: 2048,
			snapshotId: "k0123456789abcdef0123",
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		// Shown truncated (not the full opaque id), with a copy button.
		await expect(runs.getByText(/k0123456789/)).toBeVisible();
		await expect(runs.getByText("k0123456789abcdef0123")).toBeHidden();
		await expect(
			runs.getByRole("button", { name: /copy snapshot id/i }),
		).toBeVisible();
	});

	test("failed run shows expandable error detail and no upload size", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "err-group" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "err-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
			intervalSeconds: 3600,
		});
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "failure",
			error: "kopia: snapshot failed: disk quota exceeded",
			bytesUploaded: null,
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await expect(runs.getByText("err-srv")).toBeVisible();
		await expect(runs.getByText("failure")).toBeVisible();
		// A failed run uploaded nothing and has no snapshot → "—" cells (Uploaded
		// + Snapshot), not "unknown".
		await expect(runs.getByRole("cell", { name: "—" }).first()).toBeVisible();

		// Error detail is hidden until the row is expanded.
		await expect(page.getByText(/disk quota exceeded/i)).toBeHidden();
		await runs.getByRole("button", { name: /show details/i }).click();
		await expect(page.getByText(/disk quota exceeded/i)).toBeVisible();
	});

	test("run with S3 traffic but no upload size shows ~payload and expandable traffic detail", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "s3-group" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "s3-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
			intervalSeconds: 3600,
		});
		// No explicit upload size, but the proxy tallied S3 traffic → the Uploaded
		// column falls back to the payload-sent figure, marked approximate.
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "success",
			bytesUploaded: null,
			s3SentPayloadBytes: 2048, // 2.0 KiB → the Uploaded approximation
			s3SentRawBytes: 3072, // 3.0 KiB
			s3ReceivedPayloadBytes: 512, // 512 B
			s3ReceivedRawBytes: 1024, // 1.0 KiB
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await expect(runs.getByText("s3-srv")).toBeVisible();
		// Uploaded falls back to the payload-sent figure, prefixed "~".
		await expect(runs.getByText("~2.0 KiB")).toBeVisible();

		// The S3 traffic breakdown is hidden until the row is expanded.
		await expect(page.getByText(/s3 traffic/i)).toBeHidden();
		await runs.getByRole("button", { name: /show details/i }).click();
		await expect(page.getByText(/s3 traffic/i)).toBeVisible();
		await expect(page.getByText(/2\.0 KiB payload \/ 3\.0 KiB raw/i)).toBeVisible();
		await expect(page.getByText(/512 B payload \/ 1\.0 KiB raw/i)).toBeVisible();
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
		// A declared type is what enables the button (and names which type runs).
		await seedServerBackupCapability(sql, {
			serverId: server.id,
			type: "tamanu-postgres",
		});

		await page.goto(`/groups/${group.id}/backups`);
		await page.getByRole("button", { name: /backup now/i }).click();

		await expect(async () => {
			const rows = await sql.query<{ type: string }>(
				`SELECT type FROM backup_requests WHERE server_id = $1 AND purpose = 'backup'`,
				[server.id],
			);
			expect(rows).toHaveLength(1);
			expect(rows[0]!.type).toBe("tamanu-postgres");
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

	test("a server with no declared types has a disabled backup-now button", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "no-types-group" });
		await seedServer(sql, { name: "no-types-srv", groupId: group.id });
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
		});

		await page.goto(`/groups/${group.id}/backups`);
		// The server appears in the "Back up now" panel, but its button is greyed
		// out because it has registered no backup types.
		await expect(page.getByText("no-types-srv")).toBeVisible();
		await expect(
			page.getByRole("button", { name: /backup now/i }),
		).toBeDisabled();
	});

	test("a server declaring multiple types offers a backup-now per type", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "multi-type-group" });
		const server = await seedServer(sql, {
			name: "multi-srv",
			groupId: group.id,
		});
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
		});
		await seedServerBackupCapability(sql, {
			serverId: server.id,
			type: "tamanu-postgres",
		});
		await seedServerBackupCapability(sql, {
			serverId: server.id,
			type: "files",
			enabled: false,
		});

		await page.goto(`/groups/${group.id}/backups`);

		// Both declared types are listed (the disabled-for-schedule one too, since
		// a one-off backup-now overrides the enabled gate), each with its own button.
		const panel = page
			.getByRole("heading", { name: /^servers$/i })
			.locator("..");
		await expect(panel.getByText("tamanu-postgres")).toBeVisible();
		await expect(panel.getByText("files")).toBeVisible();
		await expect(
			panel.getByRole("button", { name: /backup now/i }),
		).toHaveCount(2);
		// The disabled-for-schedule type is marked, so it's clear it still has a
		// button only for on-demand use.
		await expect(panel.getByText(/not scheduled/i)).toBeVisible();

		// Backing up the non-default type writes a request for exactly that type.
		// Scope to the files row so we click that type's button, not the other's.
		await panel
			.getByRole("row")
			.filter({ hasText: "files" })
			.getByRole("button", { name: /backup now/i })
			.click();
		await expect(async () => {
			const rows = await sql.query<{ type: string }>(
				`SELECT type FROM backup_requests WHERE server_id = $1 AND purpose = 'backup'`,
				[server.id],
			);
			expect(rows).toHaveLength(1);
			expect(rows[0]!.type).toBe("files");
		}).toPass();
	});

	test("servers panel shows per-server next-backup (a lagging member isn't masked)", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "lag-group" });
		const device = await seedDevice(sql);
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
			intervalSeconds: 7200, // 2h
		});
		const ahead = await seedServer(sql, { name: "srv-ahead", groupId: group.id });
		const behind = await seedServer(sql, {
			name: "srv-behind",
			groupId: group.id,
		});
		await seedServerBackupCapability(sql, { serverId: ahead.id });
		await seedServerBackupCapability(sql, { serverId: behind.id });
		// Only `ahead` has actually backed up.
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: ahead.id,
			outcome: "success",
			bytesUploaded: 4096,
			snapshotId: "kdeadbeef0123cafe",
		});

		await page.goto(`/groups/${group.id}/backups`);
		const panel = page
			.getByRole("heading", { name: /^servers$/i })
			.locator("..");
		await expect(panel.getByText("Next backup")).toBeVisible();

		// The ahead server's next backup is in the future; it has a snapshot.
		const aheadRow = panel.getByRole("row").filter({ hasText: "srv-ahead" });
		await expect(aheadRow.getByText(/^in /)).toBeVisible();
		await expect(aheadRow.getByText(/kdeadbeef/)).toBeVisible();

		// The behind server never backed up → due now (not masked by `ahead`).
		const behindRow = panel.getByRole("row").filter({ hasText: "srv-behind" });
		// exact: the "Backup now" button also contains "now".
		await expect(behindRow.getByText("now", { exact: true })).toBeVisible();
		await expect(behindRow.getByText(/no snapshot yet/i)).toBeVisible();
	});

	test("backup page names the group and cross-links to/from the server backup section", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Linky Group" });
		const server = await seedServer(sql, {
			name: "linky-srv",
			groupId: group.id,
		});
		await seedServerBackupCapability(sql, { serverId: server.id });
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
		});

		await page.goto(`/groups/${group.id}/backups`);

		// Header carries the group name + a back-link to the group page.
		await expect(
			page.getByRole("heading", { name: /linky group backups/i }),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: /back to linky group/i }),
		).toBeVisible();

		// The server in the "back up now" panel links to its page's backup section.
		await page.getByRole("link", { name: "linky-srv" }).click();
		await expect(page).toHaveURL(new RegExp(`/servers/${server.id}#backups$`));

		// That backup section links back to the group's backup page.
		await page.getByRole("link", { name: /group backups/i }).click();
		await expect(page).toHaveURL(new RegExp(`/groups/${group.id}/backups$`));
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

	test("a capability shows its latest snapshot (id + size), copyable", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "snap-caps-group" });
		await seedServerGroupBackupConfig(sql, { groupId: group.id, status: "ready" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "snap-caps-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerBackupCapability(sql, {
			serverId: server.id,
			type: "tamanu-postgres",
		});
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "success",
			bytesUploaded: 2048,
			snapshotId: "k0123456789abcdef0123",
		});

		await page.goto(`/servers/${server.id}`);
		const backups = page.locator("#backups");
		await expect(backups.getByText(/k0123456789/)).toBeVisible();
		await expect(backups.getByText(/2\.0 KiB/)).toBeVisible();
		await expect(
			backups.getByRole("button", { name: /copy snapshot id/i }),
		).toBeVisible();
	});

	test("a capability with no successful backup shows 'no snapshot yet'", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "no-snap-group" });
		await seedServerGroupBackupConfig(sql, { groupId: group.id, status: "ready" });
		const server = await seedServer(sql, {
			name: "no-snap-srv",
			groupId: group.id,
		});
		await seedServerBackupCapability(sql, {
			serverId: server.id,
			type: "tamanu-postgres",
		});

		await page.goto(`/servers/${server.id}`);
		const backups = page.locator("#backups");
		await expect(backups.getByText(/no snapshot yet/i)).toBeVisible();
	});

	test("a recent issuance with no newer run shows 'backing up…'", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "inflight-group" });
		await seedServerGroupBackupConfig(sql, { groupId: group.id, status: "ready" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "inflight-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerBackupCapability(sql, { serverId: server.id });
		// Credentials issued 10 minutes ago, no run reported since → in flight.
		await seedBackupCredentialIssuance(sql, {
			deviceId: device.id,
			groupId: group.id,
			issuedAgoSecs: 600,
		});

		await page.goto(`/servers/${server.id}`);
		const backups = page.locator("#backups");
		await expect(backups.getByText(/backing up…/i)).toBeVisible();

		// A run reported after the issuance clears the in-flight state.
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "success",
			snapshotId: "kfreshsnap0001",
		});
		await page.reload();
		await expect(backups.getByText(/backing up…/i)).toBeHidden();
		await expect(backups.getByText(/kfreshsnap/)).toBeVisible();
	});

	test("with no group backup config, capabilities are collapsed behind a message", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "unconfigured-group" });
		const server = await seedServer(sql, {
			name: "unconfigured-srv",
			groupId: group.id,
		});
		// A declared capability, but the group has NO backup config.
		await seedServerBackupCapability(sql, {
			serverId: server.id,
			type: "tamanu-postgres",
		});

		await page.goto(`/servers/${server.id}`);
		const backups = page.locator("#backups");
		// The message explains the toggles are inert.
		await expect(backups.getByText(/aren't set up for this group/i)).toBeVisible();
		// The toggle is collapsed (not visible) until expanded.
		const toggle = backups.getByRole("switch", {
			name: /enable tamanu-postgres backups/i,
		});
		await expect(toggle).toBeHidden();
		// …but still reachable: expanding reveals it.
		await backups.getByRole("button", { name: /show backup types/i }).click();
		await expect(toggle).toBeVisible();
	});

	test("toggling a capability switch flips enabled in the DB", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "caps-group" });
		await seedServerGroupBackupConfig(sql, { groupId: group.id, status: "ready" });
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
