import { defineConfig, devices } from "@playwright/test";

// Tests assume the operator has run `just watch-private-api` (the
// private-server API on 127.0.0.1:8081) before invoking `npm run test:e2e`.
// Vite is started automatically by Playwright via the webServer block below.

export default defineConfig({
	testDir: "./e2e",
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	workers: process.env.CI ? 1 : undefined,
	reporter: [["list"]],
	timeout: 30_000,
	expect: { timeout: 5_000 },
	use: {
		baseURL: "http://localhost:8090",
		trace: "retain-on-failure",
		screenshot: "only-on-failure",
		video: "retain-on-failure",
	},
	projects: [
		{
			name: "chromium",
			use: { ...devices["Desktop Chrome"] },
		},
	],
	webServer: {
		command: "npm run dev",
		url: "http://localhost:8090",
		reuseExistingServer: !process.env.CI,
		timeout: 30_000,
	},
});
