import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedHealthcheckSeverity,
	seedIssue,
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
		const group = await seedServerGroup(sql, { name: "attention-cluster" });
		const warning = await seedServer(sql, {
			name: "warning-server",
			groupId: group.id,
		});
		const failing = await seedServer(sql, {
			name: "failing-server",
			groupId: group.id,
		});
		const healthy = await seedServer(sql, {
			name: "healthy-server",
			groupId: group.id,
		});
		const otherCheck = await seedServer(sql, {
			name: "other-check-server",
			groupId: group.id,
		});
		await seedStatus(sql, {
			serverId: warning.id,
			health: [{ check: "postgres", result: "warning" }],
		});
		await seedStatus(sql, {
			serverId: failing.id,
			health: [{ check: "postgres", result: "failed" }],
		});
		await seedStatus(sql, {
			serverId: healthy.id,
			health: [{ check: "postgres", result: "passed" }],
		});
		await seedStatus(sql, {
			serverId: otherCheck.id,
			health: [{ check: "disk_space", result: "failed" }],
		});

		await page.goto("/healthchecks/postgres");

		const failingLink = page.getByRole("link", { name: "failing-server" });
		const warningLink = page.getByRole("link", { name: "warning-server" });
		await expect(failingLink).toBeVisible();
		await expect(warningLink).toBeVisible();
		await expect(page.getByText("healthy-server")).toHaveCount(0);
		await expect(page.getByText("other-check-server")).toHaveCount(0);

		// Every row names the group too, linking to its page.
		await expect(
			page.locator(`a[href="/groups/${group.id}"]`).first(),
		).toBeVisible();

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
		const failing = await seedServer(sql, { name: "toggle-failing" });
		const healthy = await seedServer(sql, { name: "toggle-healthy" });
		await seedStatus(sql, {
			serverId: failing.id,
			health: [{ check: "postgres", result: "failed" }],
		});
		await seedStatus(sql, {
			serverId: healthy.id,
			health: [{ check: "postgres", result: "passed" }],
		});

		await page.goto("/healthchecks/postgres");
		await expect(
			page.getByRole("link", { name: "toggle-failing" }),
		).toBeVisible();
		await expect(page.getByText("toggle-healthy")).toHaveCount(0);

		await page.getByLabel(/show healthy servers/i).click();

		await expect(
			page.getByRole("link", { name: "toggle-healthy" }),
		).toBeVisible();
		await expect(page.getByText("passed", { exact: true })).toBeVisible();
		// The failing server stays listed, above the healthy one.
		const failingLink = page.getByRole("link", { name: "toggle-failing" });
		const healthyLink = page.getByRole("link", { name: "toggle-healthy" });
		const failingY = (await failingLink.boundingBox())!.y;
		const healthyY = (await healthyLink.boundingBox())!.y;
		expect(failingY).toBeLessThan(healthyY);
	});

	test("a row expands to the check's full data, like the server detail table", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "expandable-server" });
		await seedStatus(sql, {
			serverId: server.id,
			health: [
				{
					check: "postgres",
					result: "failed",
					hint: "connection refused",
					free_pct: 2,
				},
			],
		});

		await page.goto("/healthchecks/postgres");
		await expect(
			page.getByRole("link", { name: "expandable-server" }),
		).toBeVisible();
		// Collapsed: the entry's extra fields are hidden.
		await expect(page.getByText("connection refused")).toHaveCount(0);

		await page.getByRole("button", { name: /^expand$/i }).click();

		await expect(page.getByText("hint")).toBeVisible();
		await expect(page.getByText("connection refused")).toBeVisible();
		await expect(page.getByText("free_pct")).toBeVisible();
	});

	test("shows since when the check has been failing, from the active issue", async ({
		page,
		sql,
	}) => {
		const failing = await seedServer(sql, { name: "since-server" });
		const fresh = await seedServer(sql, { name: "no-issue-server" });
		await seedStatus(sql, {
			serverId: failing.id,
			health: [{ check: "postgres", result: "failed" }],
		});
		await seedStatus(sql, {
			serverId: fresh.id,
			health: [{ check: "postgres", result: "failed" }],
		});
		await seedIssue(sql, {
			serverId: failing.id,
			source: "status",
			ref: "health/postgres",
			message: "postgres check failing",
			firstSeen: new Date(Date.now() - 3 * 3_600_000).toISOString(),
		});

		await page.goto("/healthchecks/postgres");

		// The issue-backed row shows a relative failing-since; the row
		// without an issue shows the em-dash placeholder.
		await expect(page.getByText("3h ago")).toBeVisible();
		await expect(page.getByText("—")).toBeVisible();
	});

	test("shows the catalog severity when one is configured", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "sev-server" });
		await seedStatus(sql, {
			serverId: server.id,
			health: [{ check: "disk_space", result: "failed" }],
		});
		await seedHealthcheckSeverity(sql, {
			checkName: "disk_space",
			severity: "critical",
		});

		await page.goto("/healthchecks/disk_space");
		await expect(page.getByText("critical", { exact: true })).toBeVisible();
	});

	test("URL-encodes check names with special characters", async ({
		page,
		sql,
	}) => {
		const checkName = "disk space check";
		const server = await seedServer(sql, { name: "weird-check-server" });
		await seedStatus(sql, {
			serverId: server.id,
			health: [{ check: checkName, result: "failed" }],
		});

		await page.goto(`/healthchecks/${encodeURIComponent(checkName)}`);
		await expect(
			page.getByRole("heading", { name: checkName }),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: "weird-check-server" }),
		).toBeVisible();
	});

	test("server detail links a failing check to its healthcheck page", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "linked-server" });
		await seedStatus(sql, {
			serverId: server.id,
			healthy: false,
			health: [{ check: "postgres", result: "failed" }],
		});

		await page.goto(`/servers/${server.id}`);
		const checkLink = page.getByRole("link", { name: "postgres" });
		await expect(checkLink).toBeVisible();
		await checkLink.click();
		await expect(page).toHaveURL(/\/healthchecks\/postgres$/);
	});
});
