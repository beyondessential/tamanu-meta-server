import { randomUUID } from "node:crypto";

import { expect, test } from "./test-fixtures";
import {
	resetSeededTables,
	seedMachine,
	seedServer,
	seedServerGroup,
} from "./seed";

/// Every page in a deployment ends with the same picture of it: rank, then the
/// boxes at that rank, then the workloads on each box. An operator learns one
/// arrangement and reads it everywhere, and moving sideways never goes back
/// through the group.
///
/// spec: FLT
test.describe("the group's tree on the detail pages", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	/// A group with one shared box carrying two workloads and one box of its
	/// own, which is the arrangement the tree exists to show.
	async function seedTree(sql: Parameters<typeof seedServerGroup>[0]) {
		const group = await seedServerGroup(sql, { name: "tree-group" });
		const shared = await seedMachine(sql, {
			name: "shared-box",
			groupId: group.id,
		});
		const centralId = randomUUID();
		const facilityId = randomUUID();
		await sql.query(
			`INSERT INTO applications (id, name, host, type, rank, group_id, machine_id)
			 VALUES ($1, 'central-on-shared', 'https://c.e2e.invalid',
			         'tamanu-central', 'production', $3, $4),
			        ($2, 'facility-on-shared', 'https://f.e2e.invalid',
			         'tamanu-facility', 'production', $3, $4)`,
			[centralId, facilityId, group.id, shared.id],
		);
		const solo = await seedServer(sql, {
			name: "solo-app",
			groupId: group.id,
		});
		return { group, shared, centralId, facilityId, solo };
	}

	test("the application page ends with the tree, marking itself in place", async ({
		page,
		sql,
	}) => {
		const { shared, centralId, facilityId, solo } = await seedTree(sql);

		await page.goto(`/fleet/applications/${centralId}`);

		const tree = page.getByTestId("group-tree");
		await expect(tree).toBeVisible();

		// The box this workload is on, and the other box in the group, are both
		// reachable from here: the tree is a map of the deployment.
		await expect(tree.locator(`a[href="/fleet/machines/${shared.id}"]`)).toBeVisible();
		await expect(
			tree.locator(`a[href="/fleet/machines/${solo.machineId}"]`),
		).toBeVisible();

		// The workload sharing its box, and the one on the other box, are named
		// and linked.
		await expect(tree.locator(`a[href="/fleet/applications/${facilityId}"]`)).toBeVisible();
		await expect(tree.locator(`a[href="/fleet/applications/${solo.id}"]`)).toBeVisible();

		// This page is in the tree, but marked in place rather than linking back
		// to where the operator already is.
		await expect(tree.getByText("central-on-shared")).toBeVisible();
		await expect(tree.locator(`a[href="/fleet/applications/${centralId}"]`)).toHaveCount(0);
	});

	test("the machine page ends with the same tree, marking itself in place", async ({
		page,
		sql,
	}) => {
		const { shared, centralId, facilityId, solo } = await seedTree(sql);

		await page.goto(`/fleet/machines/${shared.id}`);

		const tree = page.getByTestId("group-tree");
		await expect(tree).toBeVisible();

		// Both workloads on this box, and the workload on the other one.
		await expect(tree.locator(`a[href="/fleet/applications/${centralId}"]`)).toBeVisible();
		await expect(tree.locator(`a[href="/fleet/applications/${facilityId}"]`)).toBeVisible();
		await expect(tree.locator(`a[href="/fleet/applications/${solo.id}"]`)).toBeVisible();

		// The other box links; this one is named without linking back.
		await expect(
			tree.locator(`a[href="/fleet/machines/${solo.machineId}"]`),
		).toBeVisible();
		await expect(tree.getByText("shared-box")).toBeVisible();
		await expect(tree.locator(`a[href="/fleet/machines/${shared.id}"]`)).toHaveCount(
			0,
		);
	});

	/// The title says which thing the page is about. Whether that thing is well
	/// is the tree's and the checks' business, so no dot rides alongside the
	/// name.
	///
	/// spec: FLT
	test("neither detail page shows a status dot beside its title", async ({
		page,
		sql,
	}) => {
		const { shared, centralId } = await seedTree(sql);

		await page.goto(`/fleet/applications/${centralId}`);
		await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
		await expect(
			page
				.getByRole("heading", { level: 1 })
				.locator("xpath=..")
				.getByTestId("status-dot"),
		).toHaveCount(0);

		await page.goto(`/fleet/machines/${shared.id}`);
		await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
		await expect(
			page
				.getByRole("heading", { level: 1 })
				.locator("xpath=..")
				.getByTestId("status-dot"),
		).toHaveCount(0);
	});
});
