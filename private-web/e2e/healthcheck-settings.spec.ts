import { expect, test } from "./test-fixtures";
import { resetSeededTables, seedCheckPolicy } from "./seed";

test.describe("healthcheck settings page", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	test("shows a single source-qualified 'flagging' link, not a duplicate", async ({
		page,
		sql,
	}) => {
		await seedCheckPolicy(sql, {
			checkName: "caddy_version",
			source: "alertd",
			ceiling: "warning",
		});

		await page.goto("/settings/healthchecks/caddy_version");

		// The link used to be rendered twice (a copy-paste bug). There must be
		// exactly one, and it must carry the source in its href — the settings
		// URL is check-name-only, but the "who's affected" page is keyed on the
		// (source, check) pair.
		const links = page.getByRole("link", {
			name: /See servers currently flagging this check/,
		});
		await expect(links).toHaveCount(1);
		await expect(links).toHaveAttribute(
			"href",
			"/healthchecks/alertd/caddy_version",
		);
	});

	test("admin sees editable controls (edit docs button, enabled escalate toggle)", async ({
		page,
		sql,
	}) => {
		await seedCheckPolicy(sql, {
			checkName: "caddy_version",
			source: "alertd",
			ceiling: "warning",
		});

		await page.goto("/settings/healthchecks/caddy_version");

		// The escalate toggle is an interactive, enabled switch for admins
		// (non-admins get a read-only chip instead).
		const escalate = page.getByRole("checkbox", { name: /Escalates/ });
		await expect(escalate).toBeEnabled();

		// The documentation editor's entry button is present.
		await expect(
			page.getByRole("button", { name: /Write documentation|^Edit$/ }),
		).toBeVisible();
	});
});
