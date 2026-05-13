// Shared Playwright test object. Every spec gets:
// - `stack`: a worker-scoped private-server + Vite + per-worker DB.
// - `baseURL`: wired to the stack's Vite URL (so `page.goto("/foo")`
//   just works).
// - `sql`: a worker-scoped pg client against the same per-worker DB,
//   used by the seed helpers in `seed.ts`.

import { test as base, expect } from "@playwright/test";

import { startStack, type StackHandle } from "./fixture";
import { connect, type Sql } from "./seed";

type Fixtures = {
	stack: StackHandle;
	sql: Sql;
};

export const test = base.extend<Record<string, never>, Fixtures>({
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
});

export { expect };
