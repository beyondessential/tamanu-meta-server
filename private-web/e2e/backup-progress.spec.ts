import { randomUUID } from "node:crypto";

import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedBackupCredentialIssuance,
	seedBackupRun,
	seedBackupRunProgress,
	seedDevice,
	seedServer,
	seedServerGroup,
	seedServerGroupBackupConfig,
	type Sql,
} from "./seed";

/// A group with one server whose device is mid-run: credentials issued and still
/// valid, no report yet. `runId` correlates the issuance to its progress samples.
async function seedInFlightGroup(sql: Sql, name: string) {
	const group = await seedServerGroup(sql, { name });
	const device = await seedDevice(sql);
	const server = await seedServer(sql, {
		name: `${name}-srv`,
		groupId: group.id,
		deviceId: device.id,
	});
	await seedServerGroupBackupConfig(sql, {
		groupId: group.id,
		status: "ready",
		intervalSeconds: 3600,
	});
	const runId = randomUUID();
	await seedBackupCredentialIssuance(sql, {
		deviceId: device.id,
		groupId: group.id,
		purpose: "backup",
		issuedAgoSecs: 600,
		ttlSecs: 3600,
		runId,
	});
	return { group, device, server, runId };
}

test.describe("in-flight backup progress", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("an in-flight run shows transferred, expected, rate and the freeze moment", async ({
		page,
		sql,
	}) => {
		const { group, device, server, runId } = await seedInFlightGroup(
			sql,
			"progress-live",
		);

		// Two samples 100s apart with 200 MiB between them → 2 MiB/s. Counters are
		// cumulative, so the later sample's figure is the running total, not a delta.
		const MIB = 1024 * 1024;
		await seedBackupRunProgress(sql, {
			runId,
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			observedAgoSecs: 130,
			snapshotTakenAgoSecs: 3600,
			bytesUploaded: 200 * MIB,
			bytesEstimated: 1024 * MIB,
			bytesRead: 300 * MIB,
		});
		await seedBackupRunProgress(sql, {
			runId,
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			observedAgoSecs: 30,
			bytesUploaded: 400 * MIB,
			bytesEstimated: 1024 * MIB,
			bytesRead: 600 * MIB,
			s3SentPayloadBytes: 400 * MIB,
			s3SentRawBytes: 412 * MIB,
			currentPath: "/var/lib/postgresql/base",
			extra: { engineNote: "hashing large relation" },
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await expect(runs.getByText("in progress")).toBeVisible();

		// Transferred against the run's own estimate, plus the derived rate.
		const transfer = runs.getByTestId("live-transfer");
		await expect(transfer).toContainText("400.0 MiB");
		await expect(transfer).toContainText("~1.0 GiB");
		await expect(transfer).toContainText("2.0 MiB/s");
		// 400 MiB of a 1 GiB estimate, exposed as the progress bar's own label.
		await expect(transfer.getByRole("progressbar")).toHaveAccessibleName(/39%/);

		// The freeze moment is shown on the row, distinct from the row's own time.
		await expect(runs.getByTestId("snapshot-taken")).toContainText(/data from/i);
	});

	test("an in-flight run's figures advance without a page reload", async ({
		page,
		sql,
	}) => {
		const { group, device, server, runId } = await seedInFlightGroup(
			sql,
			"progress-polling",
		);
		const MIB = 1024 * 1024;
		await seedBackupRunProgress(sql, {
			runId,
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			observedAgoSecs: 60,
			bytesUploaded: 100 * MIB,
			bytesEstimated: 1024 * MIB,
		});

		await page.goto(`/groups/${group.id}/backups`);
		const transfer = page.getByRole("table").last().getByTestId("live-transfer");
		await expect(transfer).toContainText("100.0 MiB");

		// A further sample arrives while the page is open. The panel polls at 5s
		// while any row is in flight, so the figure must move on its own — a live
		// view that only updated on reload would defeat the point.
		await seedBackupRunProgress(sql, {
			runId,
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			observedAgoSecs: 0,
			bytesUploaded: 700 * MIB,
			bytesEstimated: 1024 * MIB,
		});

		await expect(transfer).toContainText("700.0 MiB", { timeout: 20_000 });
		// And the rate becomes derivable once there are two points.
		await expect(transfer).not.toContainText(/rate unknown/i);
	});

	test("an in-flight run with no progress reported still shows as in progress", async ({
		page,
		sql,
	}) => {
		const { group } = await seedInFlightGroup(sql, "progress-absent");

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await expect(runs.getByText("in progress")).toBeVisible();
		// No figures invented, and no freeze moment claimed — absent reads as
		// unknown, not as a run stalled at zero.
		await expect(runs.getByTestId("live-transfer")).toHaveCount(0);
		await expect(runs.getByTestId("snapshot-taken")).toHaveCount(0);
	});

	test("expanding an in-flight run shows engine-vs-proxy figures and raw engine data", async ({
		page,
		sql,
	}) => {
		const { group, device, server, runId } = await seedInFlightGroup(
			sql,
			"progress-detail",
		);
		await seedBackupRunProgress(sql, {
			runId,
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			observedAgoSecs: 30,
			bytesUploaded: 400_000_000,
			bytesRead: 600_000_000,
			bytesHashed: 550_000_000,
			bytesCached: 50_000_000,
			filesDone: 12,
			filesEstimated: 40,
			s3SentPayloadBytes: 400_000_000,
			s3SentRawBytes: 412_000_000,
			extra: { engineNote: "hashing large relation" },
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await runs.getByRole("button", { name: /show details/i }).first().click();

		const detail = page.getByTestId("live-progress-detail");
		await expect(detail).toBeVisible();
		await expect(detail).toContainText(/engine vs proxy/i);
		// The proxy's raw tally exceeds its payload tally by protocol overhead.
		await expect(detail).toContainText(/overhead/i);
		await expect(detail).toContainText("12 / 40");

		// Engine detail Canopy doesn't model is available verbatim, behind a toggle.
		await expect(page.getByTestId("raw-engine-data")).toHaveCount(0);
		await page.getByRole("button", { name: /show raw engine data/i }).click();
		await expect(page.getByTestId("raw-engine-data")).toContainText(
			"hashing large relation",
		);
	});

	test("the rate chart plots a run's series, and says so when there's too little", async ({
		page,
		sql,
	}) => {
		const { group, device, server, runId } = await seedInFlightGroup(
			sql,
			"progress-chart",
		);
		// Four samples at a steady 2 MB/s, so the series has something to draw.
		for (let i = 0; i < 4; i++) {
			await seedBackupRunProgress(sql, {
				runId,
				deviceId: device.id,
				groupId: group.id,
				serverId: server.id,
				observedAgoSecs: 400 - i * 100,
				bytesUploaded: 100_000_000 + i * 200_000_000,
				bytesEstimated: 1_000_000_000,
			});
		}

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await runs.getByRole("button", { name: /show details/i }).first().click();

		await expect(page.getByTestId("throughput-chart")).toBeVisible();
		await expect(page.getByTestId("throughput-empty")).toHaveCount(0);
	});

	// The chart takes its own colour step per surface rather than flipping the
	// theme's, so it's worth proving it renders in both.
	for (const scheme of ["light", "dark"] as const) {
		test(`the rate chart renders in ${scheme} mode with a single axis unit`, async ({
			page,
			sql,
		}) => {
			const { group, device, server, runId } = await seedInFlightGroup(
				sql,
				`chart-${scheme}`,
			);
			// Ramp, plateau, a slow stretch, recovery — peaking near 31 MiB/s, so the
			// axis should top out at a round 48 with ticks in one consistent unit.
			const MIB = 1024 * 1024;
			const rates = [8, 24, 30, 31, 29, 30, 6, 4, 22, 30, 28];
			let cumulative = 0;
			for (let i = 0; i < rates.length; i++) {
				cumulative += rates[i] * 60 * MIB;
				await seedBackupRunProgress(sql, {
					runId,
					deviceId: device.id,
					groupId: group.id,
					serverId: server.id,
					observedAgoSecs: 3000 - i * 60,
					bytesUploaded: cumulative,
					bytesEstimated: 40 * 1024 * MIB,
				});
			}

			await page.emulateMedia({ colorScheme: scheme });
			await page.goto(`/groups/${group.id}/backups`);
			const runs = page.getByRole("table").last();
			await runs.getByRole("button", { name: /show details/i }).first().click();

			const chart = page.getByTestId("throughput-chart");
			await expect(chart).toBeVisible();
			const svg = chart.locator("svg");
			// The unit is stated once on the axis, and ticks are bare round numbers —
			// never one tick in MiB and the next in GiB.
			await expect(svg).toContainText("MiB/s");
			await expect(svg.getByText("48", { exact: true })).toBeVisible();
			await expect(svg.getByText("24", { exact: true })).toBeVisible();
			await expect(svg.getByText("0", { exact: true })).toBeVisible();
		});
	}

	test("a single progress sample yields no rate rather than a zero one", async ({
		page,
		sql,
	}) => {
		const { group, device, server, runId } = await seedInFlightGroup(
			sql,
			"progress-one",
		);
		await seedBackupRunProgress(sql, {
			runId,
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			observedAgoSecs: 30,
			bytesUploaded: 400_000_000,
			bytesEstimated: 1_000_000_000,
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await expect(runs.getByTestId("live-transfer")).toContainText(
			/rate unknown/i,
		);

		await runs.getByRole("button", { name: /show details/i }).first().click();
		// One point can't make a line — the chart says so rather than drawing a
		// flat zero.
		await expect(page.getByTestId("throughput-empty")).toBeVisible();
	});

	test("a completed run shows the moment its data was frozen", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "progress-reported" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "progress-reported-srv",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedServerGroupBackupConfig(sql, {
			groupId: group.id,
			status: "ready",
			intervalSeconds: 3600,
		});
		// Reported 10 minutes ago, but the data was frozen 21 hours before that —
		// the long-backup case this whole feature exists for.
		await seedBackupRun(sql, {
			deviceId: device.id,
			groupId: group.id,
			serverId: server.id,
			outcome: "success",
			bytesUploaded: 600_000_000_000,
			reportedAgoSecs: 600,
			snapshotTakenAgoSecs: 76_200,
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await expect(runs.getByText("success")).toBeVisible();
		await expect(runs.getByTestId("snapshot-taken")).toContainText(/data from/i);
		// A finished run has no live figures — its totals are on the row itself.
		await expect(runs.getByTestId("live-transfer")).toHaveCount(0);
	});

	test("a completed run that reported no freeze moment shows none", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "progress-legacy" });
		const device = await seedDevice(sql);
		const server = await seedServer(sql, {
			name: "progress-legacy-srv",
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
			bytesUploaded: 1_000,
			reportedAgoSecs: 600,
		});

		await page.goto(`/groups/${group.id}/backups`);
		const runs = page.getByRole("table").last();
		await expect(runs.getByText("success")).toBeVisible();
		await expect(runs.getByTestId("snapshot-taken")).toHaveCount(0);
	});
});
