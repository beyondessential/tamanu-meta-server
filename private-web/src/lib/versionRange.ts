/** Pretty-prints a version range pattern for display. Replaces ASCII
 * comparisons with their Unicode equivalents:
 *   `>=` → `≥`
 *   `<=` → `≤`
 *   `!=` → `≠`
 * Bare `<`, `>`, `=`, `~`, `^`, `*`, `x`, `X`, digits, dots, and dashes pass
 * through unchanged. */
export function prettifyVersionRange(pattern: string): string {
	return pattern
		.replace(/>=/g, "≥")
		.replace(/<=/g, "≤")
		.replace(/!=/g, "≠");
}
