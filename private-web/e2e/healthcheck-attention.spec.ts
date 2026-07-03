import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedHealthcheckSeverity,
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

	test("lists servers flagging the check, ordered failed before warning, and excludes others", async ({
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

		// Failed sorts above warning.
		const failingY = (await failingLink.boundingBox())!.y;
		const warningY = (await warningLink.boundingBox())!.y;
		expect(failingY).toBeLessThan(warningY);

		await failingLink.click();
		await expect(page).toHaveURL(new RegExp(`/servers/${failing.id}$`));
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
