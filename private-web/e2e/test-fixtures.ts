// Shared Playwright test object. Every spec gets:
// - `stack`: a worker-scoped private-server + Vite + per-worker DB.
// - `baseURL`: wired to the stack's Vite URL (so `page.goto("/foo")`
//   just works).
// - `sql`: a worker-scoped pg client against the same per-worker DB,
//   used by the seed helpers in `seed.ts`.
// - `nonAdminPage`: a page whose requests carry the dev non-admin header,
//   so admin-gated UI renders in its read-only, non-admin form. Use it to
//   cover the non-admin path (the default `page` is always an admin,
//   because the server's debug auth bypass grants admin by default).

import { test as base, expect, type Page } from "@playwright/test";

import { startStack, type StackHandle } from "./fixture";
import { connect, type Sql } from "./seed";

type WorkerFixtures = {
	stack: StackHandle;
	sql: Sql;
};

type TestFixtures = {
	nonAdminPage: Page;
};

export const test = base.extend<TestFixtures, WorkerFixtures>({
	stack: [
		async ({}, use) => {
			const handle = await startStack();
			try {
				await use(handle);
			} finally {
				await handle.stop();
			}
		},
		{ scope: "worker" },
	],
	baseURL: async ({ stack }, use) => {
		await use(stack.baseUrl);
	},
	sql: [
		async ({ stack }, use) => {
			const sql = await connect(stack.databaseUrl);
			try {
				await use(sql);
			} finally {
				await sql.end();
			}
		},
		{ scope: "worker" },
	],
	nonAdminPage: async ({ browser, baseURL }, use) => {
		const context = await browser.newContext({
			baseURL,
			extraHTTPHeaders: { "x-canopy-dev-non-admin": "1" },
		});
		try {
			await use(await context.newPage());
		} finally {
			await context.close();
		}
	},
});

export { expect };
