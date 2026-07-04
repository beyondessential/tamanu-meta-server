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
		await expect(page).toHaveTitle("Canopy");
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
			kind: "central",
			rank: "production",
			groupId: prodGroup.id,
		});
		await seedServer(sql, {
			name: "demo-beta",
			kind: "central",
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

	test("dot strip renders a wide group's members with rank separators", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const group = await seedServerGroup(sql, { name: "wide-fleet" });
		const device = await seedDevice(sql);

		// Twelve members across three ranks, with a spread of health states so
		// the strip shows plain, ringed, and never-reported dots. Enough dots
		// that the strip wraps at typical card widths (the screenshots artifact
		// shows the resulting grid).
		const members: Array<{
			rank: "production" | "clone" | "dev";
			kind: "central" | "facility";
			health?: unknown[];
			healthy?: boolean;
			reported?: boolean;
		}> = [
			{ rank: "production", kind: "central" },
			{ rank: "production", kind: "facility" },
			{
				rank: "production",
				kind: "facility",
				health: [{ check: "postgres", result: "failed" }],
				healthy: false,
			},
			{
				rank: "production",
				kind: "facility",
				health: [{ check: "disk_space", result: "warning" }],
			},
			{ rank: "production", kind: "facility" },
			{ rank: "production", kind: "facility", reported: false },
			{ rank: "clone", kind: "central" },
			{
				rank: "clone",
				kind: "facility",
				health: [{ check: "postgres", result: "failed" }],
				healthy: false,
			},
			{ rank: "clone", kind: "facility" },
			{ rank: "dev", kind: "central" },
			{ rank: "dev", kind: "facility" },
			{ rank: "dev", kind: "facility", reported: false },
		];
		for (const [i, m] of members.entries()) {
			const server = await seedServer(sql, {
				name: `wf-${m.rank}-${i}`,
				kind: m.kind,
				rank: m.rank,
				groupId: group.id,
			});
			if (m.reported !== false) {
				await seedStatus(sql, {
					serverId: server.id,
					deviceId: device.id,
					healthy: m.healthy ?? true,
					health: m.health ?? [],
				});
			}
		}

		await page.goto("/status");

		// The group may surface under more than one rank bucket; every card
		// shows the full strip, so scope to the first.
		const card = page.locator(`a[href="/groups/${group.id}"]`).first();
		const strip = card.getByTestId("dot-strip");
		await expect(strip).toBeVisible();
		// Two rank boundaries (production→clone, clone→dev).
		await expect(strip.getByTestId("rank-separator")).toHaveCount(2);
		// Every child cell is a dot or separator: 12 members + 2 separators.
		await expect(strip.locator("> *")).toHaveCount(14);
	});
});
