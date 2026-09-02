import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedDevice,
	seedIssue,
	seedMachine,
	seedMachineReport,
	seedServer,
	seedServerGroup,
} from "./seed";

/// The machine's own page: the box, what it reports about itself, and the
/// workloads on it.
///
/// spec: FLT
test.describe("machine detail", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("presents what the box reports about itself", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "box-group" });
		const machine = await seedMachine(sql, {
			name: "big-box",
			groupId: group.id,
		});
		await seedMachineReport(sql, {
			machineId: machine.id,
			extra: {
				hostname: "big-box.internal",
				platform: "Debian 12",
				cpuCores: 8,
				totalMemoryBytes: 34359738368,
				bestoolVersion: "2.10.5",
			},
		});

		await page.goto(`/machines/${machine.id}`);

		await expect(
			page.getByRole("heading", { level: 1, name: /big-box/ }),
		).toBeVisible();
		await expect(page.getByText("Debian 12")).toBeVisible();
		await expect(page.getByText("8", { exact: true })).toBeVisible();
		await expect(page.getByText("32.0 GiB")).toBeVisible();
		await expect(page.getByText("2.10.5")).toBeVisible();
	});

	test("a box carrying two workloads lists both, and each links to its own page", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "shared-box-group" });
		const machine = await seedMachine(sql, {
			name: "shared-box",
			groupId: group.id,
		});
		// Two workloads on the one box: the case the machine grain exists for.
		await sql.query(
			`INSERT INTO applications (id, name, host, type, rank, group_id, machine_id)
			 VALUES (gen_random_uuid(), 'central-on-shared', 'https://c.e2e.invalid',
			         'tamanu-central', 'production', $1, $2),
			        (gen_random_uuid(), 'facility-on-shared', 'https://f.e2e.invalid',
			         'tamanu-facility', 'production', $1, $2)`,
			[group.id, machine.id],
		);

		await page.goto(`/machines/${machine.id}`);

		await expect(page.getByText("Applications (2)")).toBeVisible();
		await expect(
			page.getByRole("link", { name: "central-on-shared" }),
		).toBeVisible();
		await expect(
			page.getByRole("link", { name: "facility-on-shared" }),
		).toBeVisible();
	});

	test("a box with nothing on it reads as awaiting check-in", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "empty-box-group" });
		const machine = await seedMachine(sql, {
			name: "fresh-box",
			groupId: group.id,
		});

		await page.goto(`/machines/${machine.id}`);

		await expect(page.getByText(/hasn't checked in yet/i)).toBeVisible();
		await expect(page.getByText("Applications (0)")).toBeVisible();
		await expect(page.getByText(/nothing reported on this box yet/i)).toBeVisible();
	});

	/// Enrolment admits the box, so the setup instructions live on the machine
	/// and the ticket is minted for it. Nothing runs here until the enrolled
	/// agent reports it, so no application is involved.
	///
	/// spec: FLT#machines-come-from-operators
	test("an unenrolled box offers the setup instructions and mints its ticket", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "enrol-group" });
		const machine = await seedMachine(sql, {
			name: "unenrolled-box",
			groupId: group.id,
		});

		await page.goto(`/machines/${machine.id}`);

		await expect(
			page.getByRole("heading", { name: "Set up this machine" }),
		).toBeVisible();
		await expect(page.getByText(/bestool canopy register/)).toBeVisible();
		// The ticket is the machine's: it is minted against the box's id.
		const rows = await sql.query<{ count: string }>(
			"SELECT COUNT(*) AS count FROM machine_enrollment_tokens WHERE machine_id = $1",
			[machine.id],
		);
		expect(Number(rows[0].count)).toBeGreaterThan(0);
	});

	/// The identity is bound to the box, not to any workload on it, so the
	/// device detail and re-enrolment sit behind the machine's own accordion.
	///
	/// spec: DTR
	test("the box carries the identity that speaks for it", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "identity-group" });
		const device = await seedDevice(sql, {
			tailscaleNodeName: "identity-box.tailnet.ts.net",
		});
		const machine = await seedMachine(sql, {
			name: "identity-box",
			groupId: group.id,
			deviceId: device.id,
		});
		await seedMachineReport(sql, { machineId: machine.id });

		await page.goto(`/machines/${machine.id}`);

		await page.getByRole("button", { name: "Identity" }).click();
		await expect(
			page.locator(`a[href="/devices/${device.id}"]`),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "Tailscale identity" }),
		).toBeVisible();
	});

	test("an application links to the box it runs on, and back", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "round-trip" });
		const server = await seedServer(sql, {
			name: "round-trip-app",
			groupId: group.id,
		});

		await page.goto(`/servers/${server.id}`);
		await page.getByRole("link", { name: "This box" }).click();
		await expect(page).toHaveURL(new RegExp(`/machines/${server.machineId}$`));

		await page.getByRole("link", { name: "round-trip-app" }).click();
		await expect(page).toHaveURL(new RegExp(`/servers/${server.id}$`));
	});

	test("the group lists its boxes, including one with nothing on it", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "tree-group" });
		const server = await seedServer(sql, {
			name: "tree-app",
			groupId: group.id,
		});
		const empty = await seedMachine(sql, {
			name: "tree-empty-box",
			groupId: group.id,
		});

		await page.goto(`/groups/${group.id}`);

		// Both boxes are here — the one carrying a workload and the one that has
		// not reported yet, which was invisible when the group listed workloads.
		await expect(page.getByText("Machines (2)")).toBeVisible();
		await expect(
			page.getByRole("link", { name: "tree-empty-box" }),
		).toBeVisible();
		await expect(page.getByText("Awaiting check-in.")).toBeVisible();
		// The workload sits under its box rather than beside it. Matched by its
		// own href: `seedServer` names the box after the workload, so the two
		// links share a label.
		await expect(
			page.locator(`a[href="/servers/${server.id}"]`),
		).toBeVisible();

		await page.getByRole("link", { name: "tree-empty-box" }).click();
		await expect(page).toHaveURL(new RegExp(`/machines/${empty.id}$`));
	});

	test("creating a machine lands on its own page", async ({ page, sql }) => {
		const group = await seedServerGroup(sql, { name: "landing-group" });

		await page.goto(`/groups/${group.id}/machines/new`);
		await page.getByLabel(/^Name(\s*\*)?$/i).fill("landed-box");
		await page.getByRole("button", { name: "Create machine" }).click();

		await expect(page).toHaveURL(/\/machines\/[0-9a-f-]{36}$/);
		await expect(
			page.getByRole("heading", { level: 1, name: /landed-box/ }),
		).toBeVisible();
	});

	/// A check filed against a box is silenced against that box. The scopes
	/// offered are the ones the check applies at — the machine and its group —
	/// and never one above, silencing everywhere being the check's own ceiling.
	///
	/// spec: CHK#silences-follow-the-event
	test("a machine's check can be silenced from the machine", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "silence-group" });
		const machine = await seedMachine(sql, {
			name: "noisy-box",
			groupId: group.id,
		});
		await seedIssue(sql, {
			machineId: machine.id,
			source: "alertd",
			ref: "health/disk_free",
			message: "Disk nearly full",
		});

		await page.goto(`/machines/${machine.id}`);

		// The check is here, and the scopes offered are the box and its group —
		// the ones this check applies at, and nothing above them.
		await page.getByRole("button", { name: "Silence disk_free" }).click();
		await expect(
			page.getByRole("button", { name: "For this machine" }),
		).toBeVisible();
		await expect(
			page.getByRole("button", { name: "For this group" }),
		).toBeVisible();
		await expect(
			page.getByRole("button", { name: /everywhere|fleet/i }),
		).toHaveCount(0);

		await page.getByRole("button", { name: "For this machine" }).click();

		// Wait on the section first: it renders only once the write has landed
		// and the page refetched, so it is the signal that the silence exists.
		// The popover's own text re-renders off the same fetch, and asserting on
		// it first raced that round trip under load.
		await expect(
			page.getByRole("heading", { name: /Silenced refs/ }),
		).toBeVisible();
		await expect(
			page.getByText("issues with these refs on this machine"),
		).toBeVisible();
		// And the popover now offers to lift it rather than to set it.
		await expect(page.getByText("Silenced for this machine")).toBeVisible();
	});
});
