import { defineConfig, devices } from "@playwright/test";

// Per-worker fixture (see e2e/fixture.ts) spawns its own private-server and
// Vite pair against a freshly-migrated Postgres database, so there is no
// global webServer or static baseURL here. Tests pull baseURL from the
// `stack` fixture in e2e/test-fixtures.ts.

export default defineConfig({
	testDir: "./e2e",
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	workers: process.env.CI ? 1 : undefined,
	reporter: [["list"]],
	timeout: 60_000,
	expect: { timeout: 10_000 },
	use: {
		trace: "retain-on-failure",
		screenshot: { mode: "on", fullPage: true },
		video: "retain-on-failure",
	},
	projects: [
		{
			name: "chromium",
			use: { ...devices["Desktop Chrome"] },
		},
	],
});
