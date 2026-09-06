import {
	resetSeededTables,
	seedArtifact,
	seedServerGroup,
	seedVersion,
} from "./seed";
import { expect, test } from "./test-fixtures";

/// How a version's artifacts are presented to an operator: every artifact that
/// matches, whose group each is for, and which one is actually served.
///
/// spec: ART
test.describe("group-scoped artifacts", () => {
	test.beforeEach(async ({ sql }) => {
		await resetSeededTables(sql);
	});

	/// The full set, including the artifacts specificity passed over, is
	/// available to operators: what resolution hides is a fact about how a
	/// version was published and an operator has to be able to see it.
	///
	/// spec: ART#what-a-version-offers
	test("an operator sees a group's artifact alongside the one it displaces", async ({
		page,
		sql,
	}) => {
		const version = await seedVersion(sql, {
			major: 2,
			minor: 60,
			patch: 0,
			status: "published",
		});
		const group = await seedServerGroup(sql, { name: "kamaka" });

		await seedArtifact(sql, {
			versionId: version.id,
			artifactType: "reporting-schema",
			platform: "any",
			downloadUrl: "https://example.com/all.sql",
		});
		await seedArtifact(sql, {
			versionId: version.id,
			artifactType: "reporting-schema",
			platform: "any",
			groupId: group.id,
			content: "kamaka schema",
		});

		await page.goto(`/versions/2.60.0`);

		// Both are listed, so the operator can see what the group's own
		// artifact displaced.
		const rows = page.locator("table tbody tr");
		await expect(rows).toHaveCount(2);

		// The group's artifact says whose it is and shows its digest rather
		// than a location, because Canopy holds the bytes.
		await expect(page.getByText("Held by Canopy for kamaka")).toBeVisible();
		await expect(page.getByText(/^sha256:/)).toBeVisible();

		// The unscoped one still shows where it rests.
		await expect(
			page.getByRole("link", { name: "https://example.com/all.sql" }),
		).toBeVisible();
	});

	/// A range artifact a more specific one displaces is shown, and marked as
	/// not being the one served.
	///
	/// spec: ART#what-a-version-offers
	test("an artifact resolution passed over is marked rather than hidden", async ({
		page,
		sql,
	}) => {
		const version = await seedVersion(sql, {
			major: 2,
			minor: 60,
			patch: 0,
			status: "published",
		});

		await seedArtifact(sql, {
			versionId: null,
			rangePattern: "2.60.x",
			artifactType: "installer",
			platform: "windows",
			downloadUrl: "https://example.com/broad.exe",
		});
		await seedArtifact(sql, {
			versionId: null,
			rangePattern: "^2.60.0",
			artifactType: "installer",
			platform: "windows",
			downloadUrl: "https://example.com/narrow.exe",
		});

		await page.goto(`/versions/2.60.0`);

		await expect(page.locator("table tbody tr")).toHaveCount(2);
		await expect(page.getByText("[Hidden]")).toHaveCount(1);
	});
});
