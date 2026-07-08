import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedCheckPolicy,
	seedServer,
	seedServerGroup,
	seedStatus,
} from "./seed";

test.describe("healthcheck attention page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("renders an empty state when no server flags the check", async ({
		page,
	}) => {
		await page.goto("/healthchecks/postgres");
		await expect(
			page.getByText("No servers currently flag"),
		).toBeVisible();
		await expect(page.getByRole("heading", { name: "postgres" })).toBeVisible();
	});

	test("lists servers flagging the check, ordered failed before warning, with server and group links", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Coral Coast" });
		const warning = await seedServer(sql, {
			name: "Sigatoka Facility",
			groupId: group.id,
		});
		const failing = await seedServer(sql, {
			name: "Korolevu Facility",
			groupId: group.id,
		});
		const healthy = await seedServer(sql, {
			name: "Pacific Central",
			groupId: group.id,
		});
		const otherCheck = await seedServer(sql, {
			name: "Navua Facility",
			groupId: group.id,
		});
		await seedStatus(sql, {
			serverId: warning.id,
			health: [
				{ check: "postgres", result: "warning", latency_ms: 350 },
			],
		});
		await seedStatus(sql, {
			serverId: failing.id,
			health: [
				{
					check: "postgres",
					result: "failed",
					error: "connection refused",
				},
			],
		});
		await seedStatus(sql, {
			serverId: healthy.id,
			health: [{ check: "postgres", result: "passed", latency_ms: 9 }],
		});
		await seedStatus(sql, {
			serverId: otherCheck.id,
			health: [{ check: "disk_space", result: "failed", free_pct: 3 }],
		});

		await page.goto("/healthchecks/postgres");

		const failingLink = page.getByRole("link", { name: "Korolevu Facility" });
		const warningLink = page.getByRole("link", { name: "Sigatoka Facility" });
		await expect(failingLink).toBeVisible();
		await expect(warningLink).toBeVisible();
		await expect(page.getByText("Pacific Central")).toHaveCount(0);
		await expect(page.getByText("Navua Facility")).toHaveCount(0);

		// Each row carries two links: the group's display name to the
		// group page, the server's display name to the server page.
		await expect(failingLink).toHaveAttribute(
			"href",
			`/servers/${failing.id}`,
		);
		await expect(page.locator(`a[href="/groups/${group.id}"]`)).toHaveCount(
			2,
		);
		await expect(
			page.locator(`a[href="/groups/${group.id}"]`).first(),
		).toHaveText("Coral Coast");

		// Failed sorts above warning.
		const failingY = (await failingLink.boundingBox())!.y;
		const warningY = (await warningLink.boundingBox())!.y;
		expect(failingY).toBeLessThan(warningY);

		await failingLink.click();
		await expect(page).toHaveURL(new RegExp(`/servers/${failing.id}$`));
	});

	test("the healthy-servers toggle reveals servers reporting the check passed", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Highlands" });
		const failing = await seedServer(sql, {
			name: "Mendi Facility",
			groupId: group.id,
		});
		const healthy = await seedServer(sql, {
			name: "Goroka Facility",
			groupId: group.id,
		});
		await seedStatus(sql, {
			serverId: failing.id,
			health: [
				{ check: "postgres", result: "failed", error: "timeout" },
			],
		});
		await seedStatus(sql, {
			serverId: healthy.id,
			health: [{ check: "postgres", result: "passed", latency_ms: 12 }],
		});

		await page.goto("/healthchecks/postgres");
		await expect(
			page.getByRole("link", { name: "Mendi Facility" }),
		).toBeVisible();
		await expect(page.getByText("Goroka Facility")).toHaveCount(0);

		await page.getByLabel(/show healthy servers/i).click();

		await expect(
			page.getByRole("link", { name: "Goroka Facility" }),
		).toBeVisible();
		await expect(page.getByText("passed", { exact: true })).toBeVisible();
		// The failing server stays listed, above the healthy one.
		const failingLink = page.getByRole("link", { name: "Mendi Facility" });
		const healthyLink = page.getByRole("link", { name: "Goroka Facility" });
		const failingY = (await failingLink.boundingBox())!.y;
		const healthyY = (await healthyLink.boundingBox())!.y;
		expect(failingY).toBeLessThan(healthyY);
	});

	test("a row expands to the check's full data, like the server detail table", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Vanua Levu" });
		const rich = await seedServer(sql, {
			name: "Labasa Facility",
			groupId: group.id,
		});
		const bare = await seedServer(sql, {
			name: "Savusavu Facility",
			groupId: group.id,
		});
		await seedStatus(sql, {
			serverId: rich.id,
			health: [
				{
					check: "disk_space",
					result: "failed",
					free_pct: 4,
					used_pct: 96,
					message: "volume /data almost full",
				},
			],
		});
		await seedStatus(sql, {
			serverId: bare.id,
			// A bare entry: no extra fields beyond the reserved keys.
			health: [{ check: "disk_space", result: "warning" }],
		});

		await page.goto("/healthchecks/disk_space");
		await expect(
			page.getByRole("link", { name: "Labasa Facility" }),
		).toBeVisible();
		// Collapsed: the entry's extra fields are hidden.
		await expect(page.getByText("volume /data almost full")).toHaveCount(0);

		// Expand the rich row (rows sort failed-first, so it's first).
		await page.getByRole("button", { name: /^expand$/i }).first().click();

		await expect(page.getByText("free_pct")).toBeVisible();
		await expect(page.getByText("4", { exact: true })).toBeVisible();
		await expect(page.getByText("used_pct")).toBeVisible();
		await expect(page.getByText("volume /data almost full")).toBeVisible();

		// Expand the bare row: it must say so, not render blank.
		await page.getByRole("button", { name: /^expand$/i }).click();
		await expect(
			page.getByText("No additional data reported for this check."),
		).toBeVisible();
	});

	test("shows since when the check has been failing, from the active issue", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "Western Division" });
		const failing = await seedServer(sql, {
			name: "Lautoka Facility",
			groupId: group.id,
		});
		const fresh = await seedServer(sql, {
			name: "Ba Facility",
			groupId: group.id,
		});
		await seedStatus(sql, {
			serverId: failing.id,
			health: [
				{ check: "postgres", result: "failed", error: "connection refused" },
			],
		});
		await seedStatus(sql, {
			serverId: fresh.id,
			health: [
				{ check: "postgres", result: "failed", error: "connection refused" },
			],
		});
		// The failing server's degradation streak started three hours ago;
		// the other server's state row predates the check-state stamps
		// (inactive, no streak), which renders the placeholder.
		await sql.query(
			`UPDATE issues SET degraded_since = NOW() - INTERVAL '3 hours'
			 WHERE server_id = $1 AND ref = 'health/postgres'`,
			[failing.id],
		);
		await sql.query(
			`UPDATE issues SET active = false, degraded_since = NULL
			 WHERE server_id = $1 AND ref = 'health/postgres'`,
			[fresh.id],
		);

		await page.goto("/healthchecks/postgres");

		// The state-backed row shows a relative failing-since; the row
		// without an active streak shows the em-dash placeholder.
		await expect(page.getByText("3h ago")).toBeVisible();
		await expect(page.getByText("—")).toBeVisible();
	});

	test("shows the catalog ceiling when one is configured", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "Nadi Facility" });
		await seedStatus(sql, {
			serverId: server.id,
			health: [{ check: "disk_space", result: "failed", free_pct: 2 }],
		});
		await seedCheckPolicy(sql, {
			checkName: "disk_space",
			ceiling: "failed",
			escalates: true,
		});

		await page.goto("/healthchecks/disk_space");
		// The heading chip renders the configured ceiling, plus the
		// escalates marker.
		await expect(
			page.getByRole("heading", { name: "disk_space" }),
		).toBeVisible();
		await expect(page.getByText("escalates", { exact: true })).toBeVisible();
	});

	test("URL-encodes check names with special characters", async ({
		page,
		sql,
	}) => {
		const checkName = "disk space check";
		const server = await seedServer(sql, { name: "Levuka Facility" });
		await seedStatus(sql, {
			serverId: server.id,
			health: [{ check: checkName, result: "failed" }],
		});

		await page.goto(`/healthchecks/${encodeURIComponent(checkName)}`);
		await expect(
			page.getByRole("heading", { name: checkName }),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: "Levuka Facility" }),
		).toBeVisible();
	});

	test("server detail links a failing check to its healthcheck page", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "Suva Facility" });
		await seedStatus(sql, {
			serverId: server.id,
			healthy: false,
			health: [
				{ check: "postgres", result: "failed", error: "connection refused" },
			],
		});

		await page.goto(`/servers/${server.id}`);
		const checkLink = page.getByRole("link", { name: "postgres" });
		await expect(checkLink).toBeVisible();
		await checkLink.click();
		await expect(page).toHaveURL(/\/healthchecks\/postgres$/);
	});
});
