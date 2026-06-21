import { expect, test } from "./test-fixtures";

// The fixture configures CANOPY_RECOVERY_VAULT_KEYS with a throwaway age public
// key, so the recovery vault page reports as configured. The full decrypt round-trip
// (correct answer → recorded verification) is covered by the Rust endpoint
// tests; here we exercise the page, the challenge issuance, and the
// wrong-answer rejection.
const RECIPIENT =
	"age1uy3nqmdxf4lc3sc4p32c2cp9dlqwk868gjh002gysullrgvp0cjsdg03dn";

test.describe("Recovery vault ceremony", () => {
	test("shows status, lists the recipient, and runs a challenge", async ({
		page,
	}) => {
		await page.goto("/settings/recovery");

		await expect(
			page.getByRole("heading", { name: /recovery vault/i }),
		).toBeVisible();
		// Never verified yet → due.
		await expect(page.getByText("Verification due")).toBeVisible();
		// The configured recipient is listed.
		await expect(page.getByText(RECIPIENT)).toBeVisible();

		// Issue a challenge → the ciphertext field appears, non-empty.
		await page.getByRole("button", { name: /issue challenge/i }).click();
		const challenge = page.getByLabel("Challenge (base64 age ciphertext)");
		await expect(challenge).toBeVisible();
		await expect(challenge).not.toHaveValue("");

		// A wrong answer is rejected.
		await page.getByLabel("Decrypted answer").fill("not-the-nonce");
		await page.getByRole("button", { name: /submit answer/i }).click();
		await expect(page.getByText(/does not match/i)).toBeVisible();
	});
});
