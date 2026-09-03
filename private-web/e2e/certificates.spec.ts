import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedServer,
	seedServerCertificate,
	seedServerGroup,
	seedServerGroupDomain,
	seedServerName,
} from "./seed";

// The e2e fixture runs the private-server in a debug build, so the Tailscale
// auth bypass treats every caller as `admin@localhost` (an admin), the managed
// zones are `tamanu.app` and `demo.tamanu.app`, and the certificate authority is
// the in-process fake — which advertises the `classic` and `shortlived`
// profiles and accepts revocations.

test.describe("server names and certificates", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("a server with neither grant and nothing registered shows no panel", async ({
		page,
		sql,
	}) => {
		const server = await seedServer(sql, { name: "plain" });
		await page.goto(`/applications/${server.id}`);
		// Wait for the page proper before asserting an absence, or the assertion
		// would pass against a page that simply had not loaded.
		await expect(
			page.getByRole("heading", { level: 1, name: /plain/ }),
		).toBeVisible();
		await expect(
			page.getByRole("heading", { name: "Names and certificates" }),
		).toHaveCount(0);
	});

	test("registered names show their addresses and whether the zone caught up", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageDns: true,
		});
		await seedServerName(sql, {
			serverId: server.id,
			name: "a.fiji.tamanu.app",
			addresses: ["192.0.2.1", "2001:db8::1"],
			publishedAddresses: ["192.0.2.1", "2001:db8::1"],
		});
		await seedServerName(sql, {
			serverId: server.id,
			name: "b.fiji.tamanu.app",
			addresses: ["192.0.2.9"],
			lastError: "route53 refused the change",
		});

		await page.goto(`/applications/${server.id}`);
		const panel = page
			.getByRole("heading", { name: "Names and certificates" })
			.locator("xpath=ancestor::div[contains(@class,'MuiPaper-root')][1]");

		await expect(panel.getByText("may manage DNS records")).toBeVisible();
		await expect(panel.getByText("within fiji.tamanu.app")).toBeVisible();

		await expect(panel.getByText("a.fiji.tamanu.app")).toBeVisible();
		await expect(panel.getByText("192.0.2.1, 2001:db8::1")).toBeVisible();
		await expect(panel.getByText("published", { exact: true })).toBeVisible();

		await expect(panel.getByText("b.fiji.tamanu.app")).toBeVisible();
		await expect(panel.getByText("waiting to publish")).toBeVisible();
		await expect(
			panel.getByText("route53 refused the change"),
		).toBeVisible();
	});

	test("certificates show profile, expiry, and how long is left", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageTls: true,
		});
		await seedServerCertificate(sql, {
			serverId: server.id,
			name: "a.fiji.tamanu.app",
			profile: "classic",
			lifetimeDays: 90,
			expiresInDays: 80,
		});

		await page.goto(`/applications/${server.id}`);
		const panel = page
			.getByRole("heading", { name: "Names and certificates" })
			.locator("xpath=ancestor::div[contains(@class,'MuiPaper-root')][1]");

		await expect(panel.getByText("a.fiji.tamanu.app")).toBeVisible();
		await expect(panel.getByText("valid", { exact: true })).toBeVisible();
		await expect(panel.getByText("classic", { exact: true })).toBeVisible();
		// Both readings, as the spec asks: the instant and how long is left.
		await expect(panel.getByText(/79 days left/)).toBeVisible();
	});

	test("a certificate past renewal reads as due, and one nearly gone as expiring", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageTls: true,
		});
		// A third of ninety days left is where renewal is due; a sixth is where it
		// stops being a warning.
		await seedServerCertificate(sql, {
			serverId: server.id,
			name: "due.fiji.tamanu.app",
			lifetimeDays: 90,
			expiresInDays: 20,
		});
		await seedServerCertificate(sql, {
			serverId: server.id,
			name: "gone.fiji.tamanu.app",
			lifetimeDays: 90,
			expiresInDays: 2,
		});

		await page.goto(`/applications/${server.id}`);
		const panel = page
			.getByRole("heading", { name: "Names and certificates" })
			.locator("xpath=ancestor::div[contains(@class,'MuiPaper-root')][1]");

		await expect(panel.getByText("due for renewal")).toBeVisible();
		await expect(panel.getByText("expiring", { exact: true })).toBeVisible();
	});

	test("a pending first issuance shows its reason for failing", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageTls: true,
		});
		await seedServerCertificate(sql, {
			serverId: server.id,
			name: "never.fiji.tamanu.app",
			state: "pending",
			attempts: 7,
			lastError: "the authority did not validate the name",
		});

		await page.goto(`/applications/${server.id}`);
		const panel = page
			.getByRole("heading", { name: "Names and certificates" })
			.locator("xpath=ancestor::div[contains(@class,'MuiPaper-root')][1]");

		await expect(panel.getByText("pending", { exact: true })).toBeVisible();
		await expect(
			panel.getByText(/after 7 attempt\(s\).*did not validate the name/),
		).toBeVisible();
	});

	test("the profile can be set from the authority's advertised list", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageTls: true,
		});

		await page.goto(`/applications/${server.id}`);
		const picker = page.getByLabel("Certificate lifetime");
		await expect(picker).toBeVisible();
		// The default is the authority's own, which is its longest-lived: a short
		// lifetime is adopted deliberately rather than inherited.
		await expect(
			page.getByText("Authority default (longest-lived)"),
		).toBeVisible();

		await picker.click();
		await page.getByRole("option", { name: "shortlived" }).click();

		// Polled rather than read once: the select closes before the request that
		// saves the choice has landed, so a single read races it.
		await expect
			.poll(async () => {
				const [row] = await sql.query<{ certificate_profile: string | null }>(
					"SELECT certificate_profile FROM applications WHERE id = $1",
					[server.id],
				);
				return row.certificate_profile;
			})
			.toBe("shortlived");
	});

	test("pausing records who and why, and shows what it stops", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageTls: true,
		});
		await seedServerCertificate(sql, {
			serverId: server.id,
			name: "a.fiji.tamanu.app",
		});

		await page.goto(`/applications/${server.id}`);
		await page.getByRole("button", { name: "Pause" }).click();
		await page
			.getByLabel("Reason")
			.fill("looking into an odd request pattern");
		await page
			.getByRole("button", { name: "Pause", exact: true })
			.last()
			.click();

		await expect(page.getByText("Paused")).toBeVisible();
		await expect(
			page.getByText(/looking into an odd request pattern/),
		).toBeVisible();
		await expect(page.getByText(/admin@localhost/)).toBeVisible();
		// What the pause does and does not do, said where the operator is.
		await expect(
			page.getByText(/What is already in place stands and keeps working/),
		).toBeVisible();

		// And it lifts again.
		page.once("dialog", (dialog) => dialog.accept());
		await page.getByRole("button", { name: "Resume" }).click();
		await expect(page.getByText("Paused")).toHaveCount(0);
	});

	test("revoking says it cannot be undone, and pauses the server", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageTls: true,
		});
		const cert = await seedServerCertificate(sql, {
			serverId: server.id,
			name: "leaked.fiji.tamanu.app",
		});

		await page.goto(`/applications/${server.id}`);
		await page.getByRole("button", { name: "Revoke" }).click();

		await expect(
			page.getByText(/This cannot be undone/),
		).toBeVisible();
		await expect(page.getByText(/pauses this server/)).toBeVisible();

		await page.getByLabel("Reason").click();
		await page.getByRole("option", { name: "Key compromise" }).click();
		await expect(
			page.getByText(/never be certified again, for any name by any server/),
		).toBeVisible();
		await page
			.getByRole("button", { name: "Revoke", exact: true })
			.last()
			.click();

		await expect(page.getByText("revoked", { exact: true })).toBeVisible();
		// The pause the revocation set, without being asked.
		await expect(page.getByText("Paused")).toBeVisible();

		const [row] = await sql.query<{
			state: string;
			revocation_reason: string | null;
			revoked_by: string | null;
		}>(
			"SELECT state, revocation_reason, revoked_by FROM application_certificates WHERE id = $1",
			[cert.id],
		);
		expect(row.state).toBe("revoked");
		expect(row.revocation_reason).toBe("key_compromise");
		expect(row.revoked_by).toBe("admin@localhost");

		// A compromised key is barred from ever being certified again.
		const barred = await sql.query<{ count: string }>(
			"SELECT count(*) FROM compromised_keys",
		);
		expect(Number(barred[0].count)).toBe(1);
	});
});

test.describe("group domain health", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("each domain lists the names beneath it and whether they are certified", async ({
		page,
		sql,
	}) => {
		const group = await seedServerGroup(sql, { name: "fiji" });
		await seedServerGroupDomain(sql, {
			groupId: group.id,
			domain: "fiji.tamanu.app",
		});
		const server = await seedServer(sql, {
			name: "central",
			groupId: group.id,
			mayManageDns: true,
			mayManageTls: true,
		});
		await seedServerName(sql, {
			serverId: server.id,
			name: "covered.fiji.tamanu.app",
			addresses: ["192.0.2.1"],
			publishedAddresses: ["192.0.2.1"],
		});
		await seedServerCertificate(sql, {
			serverId: server.id,
			name: "covered.fiji.tamanu.app",
		});
		// Registered but never certified, which is the case worth spotting from
		// the group's page.
		await seedServerName(sql, {
			serverId: server.id,
			name: "bare.fiji.tamanu.app",
			addresses: ["192.0.2.2"],
			publishedAddresses: ["192.0.2.2"],
		});

		await page.goto(`/groups/${group.id}`);
		const panel = page
			.getByRole("heading", { name: "Domains" })
			.locator("xpath=ancestor::div[contains(@class,'MuiPaper-root')][1]");

		await expect(panel.getByText("covered.fiji.tamanu.app")).toBeVisible();
		await expect(panel.getByText("certified")).toBeVisible();
		await expect(panel.getByText("bare.fiji.tamanu.app")).toBeVisible();
		await expect(panel.getByText("no certificate")).toBeVisible();
	});
});

test.describe("certificate authority settings", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("the authority, its profiles, and whether the account works", async ({
		page,
	}) => {
		await page.goto("/settings/certificate-authority");

		await expect(
			page.getByRole("heading", { name: "Certificate authority" }),
		).toBeVisible();
		await expect(page.getByText(/acme\.test\.invalid/)).toBeVisible();
		await expect(
			page.getByText(/Canopy holds a usable account at this authority/),
		).toBeVisible();
		await expect(page.getByText("classic", { exact: true })).toBeVisible();
		await expect(page.getByText("shortlived", { exact: true })).toBeVisible();
	});
});
