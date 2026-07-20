import { type ReactNode, createContext, useContext, useEffect, useState } from "react";
import { useApi } from "../api";
import { useReloadInterval } from "./useReloadInterval";

/** `true` if confirmed admin, `false` if confirmed non-admin, `undefined` only
 * until the very first probe resolves. Treat undefined as non-admin for gating
 * edit-only UI — better to flicker missing buttons on for an admin than to
 * briefly expose them to a non-admin. */
const AdminContext = createContext<boolean | undefined>(undefined);

/** Single source of truth for the caller's admin status. Mount once near the
 * router root so every consumer reads the same value without each one
 * re-fetching the probe. */
export function AdminProvider({ children }: { children: ReactNode }) {
	// Re-probe periodically and when the tab regains focus. The provider
	// mounts once and lives for the whole SPA session, so without this a
	// single failed probe at initial load would hide every admin control
	// until a manual hard reload.
	const tick = useReloadInterval(5 * 60_000);
	const probe = useApi("commons", "is_current_user_admin", {}, [tick]);

	// Last confirmed answer. Kept sticky across background-refetch errors so a
	// transient blip (e.g. the tailnet control plane momentarily unreachable)
	// never yanks admin controls out from under an admin mid-session; only the
	// first-ever load shows `undefined`.
	const [known, setKnown] = useState<boolean | undefined>(undefined);
	useEffect(() => {
		if (probe.status === "ok") setKnown(probe.data);
	}, [probe]);

	// Until we've learned the status at least once, retry a failed probe
	// quickly so the session self-heals without a hard reload.
	useEffect(() => {
		if (probe.status !== "error" || known !== undefined) return;
		const id = window.setTimeout(() => probe.reload(), 3000);
		return () => window.clearTimeout(id);
	}, [probe, known]);

	return <AdminContext.Provider value={known}>{children}</AdminContext.Provider>;
}

/** Returns `true`/`false` once the probe has resolved, or `undefined` while
 * loading. Callers should `=== true` to gate admin-only UI. */
export function useIsAdmin(): boolean | undefined {
	return useContext(AdminContext);
}
