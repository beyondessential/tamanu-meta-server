import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useApi } from "./api";

/** Resolve `/api/<module>/<fn>` from a per-path table of response bodies. */
function stubApi(bodies: Record<string, unknown>) {
	return vi.fn(async (input: RequestInfo | URL) => {
		const url = typeof input === "string" ? input : input.toString();
		const key = Object.keys(bodies).find((k) => url.includes(k));
		return new Response(JSON.stringify(key ? bodies[key] : {}), {
			status: 200,
			headers: { "content-type": "application/json" },
		});
	});
}

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("useApi keeps prior data only for the same query", () => {
	it("goes to loading when deps change to a different entity", async () => {
		// One slow endpoint, so the window between "deps changed" and "new data
		// arrived" is observable — that window is where the previous entity's
		// data used to stay on screen under the new entity's URL.
		let release: (() => void) | undefined;
		const gate = new Promise<void>((r) => {
			release = r;
		});
		let call = 0;
		vi.stubGlobal(
			"fetch",
			vi.fn(async () => {
				call += 1;
				const which = call;
				if (which === 2) await gate;
				return new Response(JSON.stringify({ name: `server-${which}` }), {
					status: 200,
					headers: { "content-type": "application/json" },
				});
			}),
		);

		const { result, rerender } = renderHook(
			({ id }: { id: string }) =>
				// Cast: these name a stub endpoint, not a real one in the spec.
				useApi("servers" as any, "get_detail" as any, { id }, [id]),
			{ initialProps: { id: "A" } },
		);

		await waitFor(() => expect(result.current.status).toBe("ok"));
		expect((result.current as { data: { name: string } }).data.name).toBe(
			"server-1",
		);

		rerender({ id: "B" });

		await waitFor(() =>
			expect(result.current.status).toBe("loading"),
		);
		expect(result.current).not.toHaveProperty("data");

		release?.();
		await waitFor(() => expect(result.current.status).toBe("ok"));
		expect((result.current as { data: { name: string } }).data.name).toBe(
			"server-2",
		);
	});

	// Callers force a refetch by bumping a nonce inside `deps` (`[id, tick]`).
	// That's the same entity, so the page must not blank — anything with local
	// state inside it (an open dialog mid-flow, say) would be unmounted.
	it("keeps prior data when only a refetch nonce in deps changes", async () => {
		vi.stubGlobal("fetch", stubApi({ "/api/": { name: "same" } }));

		const { result, rerender } = renderHook(
			({ tick }: { tick: number }) =>
				// Cast: these name a stub endpoint, not a real one in the spec.
				useApi("devices" as any, "get_device_by_id" as any, { device_id: "A" }, [
					"A",
					tick,
				]),
			{ initialProps: { tick: 0 } },
		);
		await waitFor(() => expect(result.current.status).toBe("ok"));

		rerender({ tick: 1 });
		expect(result.current.status).toBe("ok");
		await waitFor(() => expect(result.current.status).toBe("ok"));
	});

	it("keeps prior data across a reload of the same query", async () => {
		vi.stubGlobal("fetch", stubApi({ "/api/": { name: "same" } }));

		const { result } = renderHook(() =>
			// Cast: these name a stub endpoint, not a real one in the spec.
			useApi("servers" as any, "get_detail" as any, { id: "A" }, ["A"]),
		);
		await waitFor(() => expect(result.current.status).toBe("ok"));

		result.current.reload();
		// A background refetch must not collapse the page to a placeholder.
		expect(result.current.status).toBe("ok");
	});
});
