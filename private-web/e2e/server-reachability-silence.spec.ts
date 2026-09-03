import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServer,
	seedServerGroup,
	seedServerSilencedRef,
	seedStatus,
	seedVersion,
	type Sql,
} from "./seed";

/** The machine-scoped silences on a box's reachability check — what the
 * machine form's switch writes. */
async function machineReachabilitySilences(
	sql: Sql,
	machineId: string,
): Promise<number> {
	const rows = await sql.query<{ n: string }>(
		"SELECT COUNT(*) AS n FROM scoped_check_policies \
		 WHERE machine_id = $1 AND source = 'canopy' AND check_name = 'reachability' \
		 AND ceiling = 'skipped'",
		[machineId],
	);
	return Number(rows[0]!.n);
}

const MACHINE_SWITCH = "Alert when this machine is unreachable";

/** The server-scoped silences on a server's reachability check — what the
 * form's "alert when this server is unreachable" switch writes, and what
 * the check's own silence button writes. */
async function reachabilitySilences(sql: Sql, serverId: string): Promise<number> {
	const rows = await sql.query<{ n: string }>(
		"SELECT COUNT(*) AS n FROM scoped_check_policies \
		 WHERE application_id = $1 AND source = 'canopy' AND check_name = 'reachability' \
		 AND ceiling = 'skipped'",
		[serverId],
	);
	return Number(rows[0]!.n);
}

const SWITCH = "Alert when this server is unreachable";

/** The box's switch, offered on the application so an operator quieting a
 * host expected to be down doesn't have to find which record owns it. */
const BOX_SWITCH = "Alert when the machine this runs on is unreachable";

// spec: CHK#operator-controls
test.describe("the reachability alerting switch, on both forms", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("creating a machine with the switch off silences its reachability", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const group = await seedServerGroup(sql, { name: "comes-and-goes" });

		await page.goto(`/groups/${group.id}/machines/new`);
		await page.getByLabel(/^Name(\s*\*)?$/i).fill("expected-to-vanish");
		// On by default: a new box alerts when it goes away unless told not to.
		await expect(page.getByLabel(MACHINE_SWITCH)).toBeChecked();
		await page.getByLabel(MACHINE_SWITCH).uncheck();
		await page.getByRole("button", { name: "Create machine" }).click();

		await expect(page).toHaveURL(/\/machines\/[0-9a-f-]{36}$/);
		const id = page.url().split("/").pop()!;
		expect(await machineReachabilitySilences(sql, id)).toBe(1);
	});

	test("creating a machine with the switch on leaves reachability alerting", async ({
		page,
		sql,
	}) => {
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const group = await seedServerGroup(sql, { name: "should-stay-up" });

		await page.goto(`/groups/${group.id}/machines/new`);
		await page.getByLabel(/^Name(\s*\*)?$/i).fill("expected-to-stay");
		// The switch settling is what says the form is live; clicking before
		// that lands on markup with no handler attached yet.
		await expect(page.getByLabel(MACHINE_SWITCH)).toBeChecked();
		await page.getByRole("button", { name: "Create machine" }).click();

		await expect(page).toHaveURL(/\/machines\/[0-9a-f-]{36}$/);
		const id = page.url().split("/").pop()!;
		expect(await machineReachabilitySilences(sql, id)).toBe(0);
	});

	test("the edit form reflects an existing silence and clearing it re-enables alerting", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "edit-reach-group" });
		const server = await seedServer(sql, {
			name: "already-hushed",
			groupId: group.id,
		});
		// As the check's own silence button would have left it.
		await seedServerSilencedRef(sql, {
			serverId: server.id,
			source: "canopy",
			ref: "reachability",
		});

		await page.goto(`/applications/${server.id}/edit`);
		// The switch reads the silence, so the form doesn't quietly write the
		// operator's earlier decision away on the next unrelated save.
		await expect(page.getByLabel(SWITCH)).not.toBeChecked();

		await page.getByLabel(SWITCH).check();
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/applications/${server.id}`);

		expect(await reachabilitySilences(sql, server.id)).toBe(0);
	});

	test("turning the switch off from the edit form silences reachability", async ({
		page,
		sql,
	}) => {
		// Seeded with a version and a status so the detail page it lands on
		// renders the checks table we then assert against.
		await seedVersion(sql, { major: 1, minor: 0, patch: 0 });
		const group = await seedServerGroup(sql, { name: "edit-hush-group" });
		const server = await seedServer(sql, {
			name: "about-to-be-hushed",
			groupId: group.id,
		});
		await seedStatus(sql, { serverId: server.id, healthy: true });

		await page.goto(`/applications/${server.id}/edit`);
		await expect(page.getByLabel(SWITCH)).toBeChecked();
		await page.getByLabel(SWITCH).uncheck();
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/applications/${server.id}`);

		expect(await reachabilitySilences(sql, server.id)).toBe(1);
		// And the check itself now shows as silenced, so the two surfaces agree.
		await expect(page.getByText("silenced (application)")).toBeVisible();
	});

	// spec: CHK#operator-controls
	test("the box's switch is offered on an application, and quiets only the box", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "box-switch-group" });
		const server = await seedServer(sql, {
			name: "on-a-box-going-away",
			groupId: group.id,
		});

		await page.goto(`/applications/${server.id}/edit`);
		await expect(page.getByLabel(BOX_SWITCH)).toBeChecked();
		await page.getByLabel(BOX_SWITCH).uncheck();
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/applications/${server.id}`);

		// The box is quiet, and the application it carries is not: each grain
		// has its own reachability and its own switch.
		expect(await machineReachabilitySilences(sql, server.machineId)).toBe(1);
		expect(await reachabilitySilences(sql, server.id)).toBe(0);

		// And the box's own page reads the same state back.
		await page.goto(`/machines/${server.machineId}`);
		await expect(
			page.getByText(
				"— issues with these refs on this machine don't open incidents.",
			),
		).toBeVisible();
		await expect(page.getByText("reachability").first()).toBeVisible();
	});

	// spec: CHK#reachability
	test("silencing an application's reachability leaves its box alerting", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "app-only-hush-group" });
		const server = await seedServer(sql, {
			name: "workload-expected-to-stop",
			groupId: group.id,
		});

		await page.goto(`/applications/${server.id}/edit`);
		await expect(page.getByLabel(SWITCH)).toBeChecked();
		await page.getByLabel(SWITCH).uncheck();
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/applications/${server.id}`);

		expect(await reachabilitySilences(sql, server.id)).toBe(1);
		expect(await machineReachabilitySilences(sql, server.machineId)).toBe(0);
	});

	test("saving with the switch untouched leaves the silence alone", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "untouched-group" });
		const server = await seedServer(sql, {
			name: "leave-me-be",
			groupId: group.id,
		});
		await seedServerSilencedRef(sql, {
			serverId: server.id,
			source: "canopy",
			ref: "reachability",
		});

		await page.goto(`/applications/${server.id}/edit`);
		await page.getByLabel(/^Name(\s*\*)?$/i).fill("leave-me-be-renamed");
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/applications/${server.id}`);

		expect(await reachabilitySilences(sql, server.id)).toBe(1);
	});
});
