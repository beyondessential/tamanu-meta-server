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

		// `2.60.x` is >=2.60.0 <2.61.0 and `^2.60.0` is >=2.60.0 <3.0.0, so the
		// wildcard is the narrower of the two and is the one served.
		await seedArtifact(sql, {
			versionId: null,
			rangePattern: "2.60.x",
			artifactType: "installer",
			platform: "windows",
			downloadUrl: "https://example.com/narrow.exe",
		});
		await seedArtifact(sql, {
			versionId: null,
			rangePattern: "^2.60.0",
			artifactType: "installer",
			platform: "windows",
			downloadUrl: "https://example.com/wide.exe",
		});

		await page.goto(`/versions/2.60.0`);

		await expect(page.locator("table tbody tr")).toHaveCount(2);
		await expect(page.getByText("[Hidden]")).toHaveCount(1);

		// Counting the markers alone would pass just as well with the marker on
		// the artifact that is actually served.
		const wide = page
			.locator("table tbody tr")
			.filter({ hasText: "https://example.com/wide.exe" });
		await expect(wide.getByText("[Hidden]")).toBeVisible();

		const narrow = page
			.locator("table tbody tr")
			.filter({ hasText: "https://example.com/narrow.exe" });
		await expect(narrow.getByText("[Hidden]")).toHaveCount(0);
	});

	/// A group's artifact displaces the unscoped one for that group alone, so
	/// the unscoped one is still what every other caller is served and is not
	/// marked as passed over. Resolving once across the fleet marks it hidden,
	/// which tells the operator the opposite of the truth.
	///
	/// spec: ART#what-a-version-offers
	test("an artifact a group's own displaces is not marked hidden", async ({
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
			versionId: null,
			rangePattern: "2.60.x",
			artifactType: "reporting-schema",
			platform: "any",
			downloadUrl: "https://example.com/fleet.sql",
		});
		await seedArtifact(sql, {
			versionId: version.id,
			artifactType: "reporting-schema",
			platform: "any",
			groupId: group.id,
			content: "kamaka schema",
		});

		await page.goto(`/versions/2.60.0`);

		await expect(page.locator("table tbody tr")).toHaveCount(2);
		await expect(page.getByText("[Hidden]")).toHaveCount(0);
	});


	/// An operator publishes into a group by carrying the bytes: there is no
	/// store to be credentialled for, so being able to register for the group is
	/// the whole of what publishing into it takes.
	///
	/// spec: ART#where-an-artifact-rests
	test("an operator registers a group's artifact by uploading it", async ({
		page,
		sql,
	}) => {
		const version = await seedVersion(sql, {
			major: 2,
			minor: 60,
			patch: 0,
			status: "published",
		});
		await seedServerGroup(sql, { name: "kamaka" });

		await page.goto(`/versions/2.60.0`);
		await page.getByRole("button", { name: "Unlock" }).click();
		await page.getByRole("button", { name: "Create" }).click();

		await page.getByRole("textbox", { name: "Type" }).fill("reporting-schema");
		await page.getByRole("textbox", { name: "Platform" }).fill("any");
		await page.getByRole("combobox", { name: "Group" }).click();
		await page.getByRole("option", { name: "kamaka" }).click();
		await page.getByLabel("Choose file…").setInputFiles({
			name: "kamaka.sql",
			mimeType: "application/sql",
			buffer: Buffer.from("kamaka schema"),
		});
		// The submit shares its label with the button that revealed the form,
		// so it has to be picked out of the form itself.
		await page
			.locator("form")
			.getByRole("button", { name: "Create" })
			.click();

		await expect(page.getByText("Held by Canopy for kamaka")).toBeVisible();

		// Canopy holds the bytes and records the digest of what it took in.
		const rows = await sql.query<{
			download_url: string | null;
			digest: string | null;
			content: string | null;
		}>(
			`SELECT download_url, digest, encode(content, 'escape') AS content
			 FROM artifacts WHERE version_id = $1`,
			[version.id],
		);
		expect(rows).toHaveLength(1);
		expect(rows[0].download_url).toBeNull();
		expect(rows[0].content).toBe("kamaka schema");
		expect(rows[0].digest).toBe(
			"sha256:214b3ad41c660e2837e03418fe87c70b1e82cc7c3531d78efeff9a3409ea91d9",
		);
	});

	/// An artifact Canopy holds has no location to edit. Replacing its bytes is
	/// a registration, which is what carries the digest.
	///
	/// spec: ART#where-an-artifact-rests
	test("a held artifact offers no location to edit", async ({ page, sql }) => {
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
			groupId: group.id,
			content: "kamaka schema",
		});

		await page.goto(`/versions/2.60.0`);
		await page.getByRole("button", { name: "Unlock" }).click();
		await page
			.getByRole("button", { name: "edit reporting-schema any for kamaka" })
			.click();

		await expect(
			page.getByText("Register it again to replace the bytes"),
		).toBeVisible();
	});

	/// Canopy keeps none of what it has stopped serving, so removing an artifact
	/// takes the bytes it held with it.
	///
	/// spec: ART#where-an-artifact-rests
	test("deleting a group's artifact takes its bytes", async ({ page, sql }) => {
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
		await page.getByRole("button", { name: "Unlock" }).click();

		// The two rows share a type and platform, so the label has to name the
		// group to pick one out.
		await page
			.getByRole("button", { name: "delete reporting-schema any for kamaka" })
			.click();
		await page.getByRole("button", { name: "Really delete" }).click();

		await expect(page.getByText("Held by Canopy for kamaka")).toHaveCount(0);

		const [held] = await sql.query<{ n: string }>(
			"SELECT count(*) AS n FROM artifacts WHERE content IS NOT NULL",
		);
		expect(Number(held.n)).toBe(0);

		// The unscoped one is untouched.
		const [left] = await sql.query<{ n: string }>(
			"SELECT count(*) AS n FROM artifacts",
		);
		expect(Number(left.n)).toBe(1);
	});
});
