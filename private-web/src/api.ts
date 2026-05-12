import { useCallback, useEffect, useRef, useState } from "react";

import type { ApiFn, ApiModule } from "./types";

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
// (module, fn) pair against the generated `paths` interface, so passing an
// unknown endpoint is a compile-time error. The response type `T` stays
// caller-provided for now — auto-inferring it from the path would force
// every call site to drop its explicit generic, which is a follow-up.
export async function callApi<
	T,
	M extends ApiModule = ApiModule,
	F extends ApiFn<M> = ApiFn<M>,
>(
	module: M,
	fn: F,
	params: Record<string, unknown> = {},
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
		throw new ApiError(
			response.status,
			`server fn ${module}.${fn} failed: ${response.status}`,
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

export function useApi<
	T,
	M extends ApiModule = ApiModule,
	F extends ApiFn<M> = ApiFn<M>,
>(
	module: M,
	fn: F,
	params: Record<string, unknown> = {},
	deps: ReadonlyArray<unknown> = [],
): ApiState<T> & { reload: () => void } {
	const [state, setState] = useState<ApiState<T>>({ status: "idle" });
	const tick = useRef(0);

	const run = useCallback(() => {
		const myTick = ++tick.current;
		const controller = new AbortController();
		setState({ status: "loading" });
		callApi<T, M, F>(module, fn, params, controller.signal)
			.then((data) => {
				if (tick.current === myTick) setState({ status: "ok", data });
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
	T = void,
	M extends ApiModule = ApiModule,
	F extends ApiFn<M> = ApiFn<M>,
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
				const result = await callApi<T, M, F>(module, fn, params);
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
