// Shared Playwright test object: every spec gets a worker-scoped `stack` that
// points at a per-worker private-server + Vite pair, and Playwright's
// built-in `baseURL` is wired through it so existing `page.goto("/admins")`
// calls still resolve transparently.

import { test as base, expect } from "@playwright/test";

import { startStack, type StackHandle } from "./fixture";

type Fixtures = {
	stack: StackHandle;
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
});

export { expect };
