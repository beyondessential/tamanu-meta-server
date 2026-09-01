import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedDevice,
	seedServer,
	seedServerGroup,
	seedStatus,
	seedVersion,
} from "./seed";

test.describe("status page", () => {
	test.beforeEach(async ({ sql }) => {
		// Each test inside this file makes assertions about either
		// emptiness or specific seeded rows, so wipe state on entry.
		// Worker-scoped fixtures otherwise leak between tests/specs.
		await resetSeededTables(sql);
	});

	test("renders the nav and redirects /", async ({ page }) => {
		await page.goto("/");
		await expect(page).toHaveURL(/\/status$/);
		await expect(page).toHaveTitle("Status · Canopy");
		await expect(page.getByRole("link", { name: "Status" })).toBeVisible();
	});

	test("empty database surfaces an info banner", async ({ page }) => {
		await page.goto("/status");
		await expect(page.getByText(/no server groups configured/i)).toBeVisible();
	});

	test("groups by rank bucket and links to each group's detail page", async ({
		page,
		sql,
	}) => {
		// group_details computes version-distance against the latest
		// published version; without one the card 404s.
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const prodGroup = await seedServerGroup(sql, { name: "prod-cluster" });
		const demoGroup = await seedServerGroup(sql, { name: "demo-cluster" });
		await seedServer(sql, {
			name: "prod-alpha",
			type: "tamanu-central",
			rank: "production",
			groupId: prodGroup.id,
		});
		await seedServer(sql, {
			name: "demo-beta",
			type: "tamanu-central",
			rank: "demo",
			groupId: demoGroup.id,
		});

		await page.goto("/status");

		// Rank section headings.
		await expect(
			page.getByRole("heading", { name: "production", exact: true }),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "demo", exact: true }),
		).toBeVisible();

		// Group names render as h3 inside each card.
		await expect(
			page.getByRole("heading", { name: prodGroup.name }),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: demoGroup.name }),
		).toBeVisible();

		// Each card is wrapped in a router link to /groups/<id>.
		await expect(
			page.locator(`a[href="/groups/${prodGroup.id}"]`),
		).toBeVisible();
	});

	test("dot strips wrap and align across a spread of group sizes", async ({
		page,
		sql,
	}) => {
		test.setTimeout(120_000);
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const device = await seedDevice(sql);

		// Groups of increasing size so the strip renders unwrapped, wrapped
		// once, and wrapped many times (the screenshots artifact shows the
		// grids). Each group spans three ranks, so two triangle separators
		// appear per strip; health states cycle so plain, failed-ringed,
		// warning-ringed, and never-reported dots all show up.
		const splits: Record<number, [number, number, number]> = {
			5: [3, 1, 1],
			10: [6, 2, 2],
			20: [12, 5, 3],
			30: [18, 8, 4],
			50: [30, 13, 7],
		};
		const groups: Array<{ id: string; size: number }> = [];
		for (const [size, split] of Object.entries(splits)) {
			const n = Number(size);
			const group = await seedServerGroup(sql, { name: `fleet-of-${n}` });
			groups.push({ id: group.id, size: n });
			const ranks = (["production", "clone", "dev"] as const).flatMap(
				(rank, ri) => Array(split[ri]).fill(rank) as ("production" | "clone" | "dev")[],
			);
			for (const [i, rank] of ranks.entries()) {
				const server = await seedServer(sql, {
					name: `f${n}-${i}`,
					kind: i === 0 ? "central" : "facility",
					rank,
					groupId: group.id,
				});
				if (i % 7 === 6) continue; // never reported: grey dot
				const health =
					i % 4 === 3
						? [{ check: "postgres", result: "failed" }]
						: i % 5 === 4
							? [{ check: "disk_space", result: "warning" }]
							: [];
				await seedStatus(sql, {
					serverId: server.id,
					deviceId: device.id,
					healthy: i % 4 !== 3,
					health,
				});
			}
		}

		await page.goto("/status");

		for (const { id, size } of groups) {
			// A group may surface under more than one rank bucket; every card
			// shows the full strip, so scope to the first.
			const card = page.locator(`a[href="/groups/${id}"]`).first();
			const strip = card.getByTestId("dot-strip");
			await expect(strip).toBeVisible();
			// Two rank boundaries (production→clone, clone→dev).
			await expect(strip.getByTestId("rank-separator")).toHaveCount(2);
			// Every child cell is a dot or a separator.
			await expect(strip.locator("> *")).toHaveCount(size + 2);
		}
	});

	// spec: FIG#active-versions
	test("counts the release branches production is actively running", async ({
		page,
		sql,
	}) => {
		const live = await seedServer(sql, {
			name: "prod-live",
			rank: "production",
		});
		const quiet = await seedServer(sql, {
			name: "prod-quiet",
			rank: "production",
		});
		const testing = await seedServer(sql, { name: "test-box", rank: "test" });

		await seedStatus(sql, { serverId: live.id, version: "2.34.1" });
		// A later push from a source carrying no version must not drop the
		// server out of the summary.
		await seedStatus(sql, {
			serverId: live.id,
			source: "tamanu",
			extra: { uptimeSecs: 42 },
		});
		// Quiet for longer than a week: not running anything, as far as the
		// summary is concerned.
		await seedStatus(sql, {
			serverId: quiet.id,
			version: "2.10.0",
			createdAt: "NOW() - INTERVAL '8 days'",
		});
		await seedStatus(sql, { serverId: testing.id, version: "2.40.0" });

		await page.goto("/status");

		await expect(
			page.getByText(
				"1 release branch in active use: 2.34 (1 version: 2.34.1 — 2.34.1)",
			),
		).toBeVisible();

		// The card answers "which branches"; the figures page answers "which
		// servers, and what else are they running".
		await page.getByRole("link", { name: "Fleet figures" }).click();
		await expect(page).toHaveURL(/\/servers\/figures$/);
		await expect(page.getByRole("group", { name: "Tamanu" })).toBeVisible();
	});
});
