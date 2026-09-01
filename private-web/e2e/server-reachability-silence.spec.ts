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

// spec: CHK#operator-controls
test.describe("reachability alerting switch on the server form", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	// A machine-create form carries no reachability switch: a machine's
	// reachability silence would have to be machine-scoped, and silences are
	// still application-scoped. Creating a box and quieting it is two steps
	// until that exists.

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

		await page.goto(`/servers/${server.id}/edit`);
		// The switch reads the silence, so the form doesn't quietly write the
		// operator's earlier decision away on the next unrelated save.
		await expect(page.getByLabel(SWITCH)).not.toBeChecked();

		await page.getByLabel(SWITCH).check();
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/servers/${server.id}`);

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

		await page.goto(`/servers/${server.id}/edit`);
		await expect(page.getByLabel(SWITCH)).toBeChecked();
		await page.getByLabel(SWITCH).uncheck();
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/servers/${server.id}`);

		expect(await reachabilitySilences(sql, server.id)).toBe(1);
		// And the check itself now shows as silenced, so the two surfaces agree.
		await expect(page.getByText("silenced (server)")).toBeVisible();
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

		await page.goto(`/servers/${server.id}/edit`);
		await page.getByLabel(/^Name(\s*\*)?$/i).fill("leave-me-be-renamed");
		await page.getByRole("button", { name: /^save$/i }).click();
		await page.waitForURL(`**/servers/${server.id}`);

		expect(await reachabilitySilences(sql, server.id)).toBe(1);
	});
});
