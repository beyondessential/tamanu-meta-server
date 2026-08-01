import { useCallback, useEffect, useRef, useState } from "react";

import type { ApiBody, ApiFn, ApiModule, ApiResponse } from "./types";

export class ApiError extends Error {
	readonly status: number;
	readonly detail: unknown;

	constructor(status: number, message: string, detail: unknown) {
		super(message);
		this.name = "ApiError";
		this.status = status;
		this.detail = detail;
	}
}

// `M` and `F` are inferred from the positional args and constrain the
// (module, fn) pair against the generated `paths` interface. `T` defaults to
// the response type for that path, so most call sites don't need to spell it
// out — they only need to override when narrowing or shaping locally.
export async function callApi<
	M extends ApiModule,
	F extends ApiFn<M>,
	T = ApiResponse<M, F>,
>(
	module: M,
	fn: F,
	params: ApiBody<M, F> | Record<string, unknown> = {},
	signal?: AbortSignal,
): Promise<T> {
	const response = await fetch(`/api/${module}/${fn}`, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify(params),
		signal,
	});

	if (!response.ok) {
		let detail: unknown = null;
		try {
			detail = await response.json();
		} catch {
			detail = await response.text().catch(() => null);
		}
		// Surface the problem-details title (and detail line, if present)
		// in the thrown error's message so action.error?.message in the UI
		// shows the actual server-side cause, not just the HTTP status.
		let extra = "";
		if (
			detail &&
			typeof detail === "object" &&
			"title" in detail &&
			typeof (detail as { title?: unknown }).title === "string"
		) {
			extra = `: ${(detail as { title: string }).title}`;
		}
		throw new ApiError(
			response.status,
			`server fn ${module}.${fn} failed: ${response.status}${extra}`,
			detail,
		);
	}

	return (await response.json()) as T;
}

export type ApiState<T> =
	| { status: "idle" }
	| { status: "loading" }
	| { status: "ok"; data: T }
	| { status: "error"; error: Error };

/** Element-wise `Object.is` comparison; `deps` arrays are new on each render. */
function sameDeps(
	a: ReadonlyArray<unknown>,
	b: ReadonlyArray<unknown>,
): boolean {
	return a.length === b.length && a.every((v, i) => Object.is(v, b[i]));
}

export function useApi<
	M extends ApiModule,
	F extends ApiFn<M>,
	T = ApiResponse<M, F>,
>(
	module: M,
	fn: F,
	params: Record<string, unknown> = {},
	deps: ReadonlyArray<unknown> = [],
): ApiState<T> & { reload: () => void } {
	const [state, setState] = useState<ApiState<T>>({ status: "idle" });
	const tick = useRef(0);
	// The `deps` that produced the data currently on screen, so a background
	// refetch can be told apart from a switch to a different entity.
	const shownDeps = useRef<ReadonlyArray<unknown> | null>(null);

	const run = useCallback(() => {
		const myTick = ++tick.current;
		const controller = new AbortController();
		// Keep prior data on screen during background refetches so the UI
		// doesn't collapse to a loading placeholder on every reload tick —
		// but only when it's the *same* query. Detail routes reuse the mounted
		// component across /servers/A → /servers/B, so holding on to prior
		// data across a deps change renders A's name, health and checks under
		// B's URL, with no loading indicator to say otherwise.
		const sameQuery =
			shownDeps.current !== null && sameDeps(shownDeps.current, deps);
		setState((prev) =>
			prev.status === "ok" && sameQuery ? prev : { status: "loading" },
		);
		callApi<M, F, T>(module, fn, params, controller.signal)
			.then((data) => {
				if (tick.current === myTick) {
					shownDeps.current = deps;
					setState({ status: "ok", data });
				}
			})
			.catch((error: unknown) => {
				if (controller.signal.aborted) return;
				if (tick.current !== myTick) return;
				setState({
					status: "error",
					error: error instanceof Error ? error : new Error(String(error)),
				});
			});
		return () => controller.abort();
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, deps);

	useEffect(() => run(), [run]);

	return { ...state, reload: run };
}

/**
 * Hook for write/mutation server fns. Returns a `call` function that
 * performs the request and a `pending` / `error` state for the UI.
 * The caller decides what to do with the result (e.g. refetch a
 * `useApi` resource).
 */
export function useApiAction<
	M extends ApiModule,
	F extends ApiFn<M>,
	T = ApiResponse<M, F>,
>(
	module: M,
	fn: F,
): {
	call: (params?: Record<string, unknown>) => Promise<T>;
	pending: boolean;
	error: Error | null;
	reset: () => void;
} {
	const [pending, setPending] = useState(false);
	const [error, setError] = useState<Error | null>(null);

	const call = useCallback(
		async (params: Record<string, unknown> = {}): Promise<T> => {
			setPending(true);
			setError(null);
			try {
				const result = await callApi<M, F, T>(module, fn, params);
				// Broadcast so global, page-agnostic queries (e.g. the open-
				// incidents nav badge) can refetch without the caller having
				// to know they exist. Listeners hook via useReloadInterval.
				document.dispatchEvent(new Event("canopy-data-changed"));
				return result;
			} catch (err) {
				const e = err instanceof Error ? err : new Error(String(err));
				setError(e);
				throw e;
			} finally {
				setPending(false);
			}
		},
		[module, fn],
	);

	const reset = useCallback(() => setError(null), []);

	return { call, pending, error, reset };
}
