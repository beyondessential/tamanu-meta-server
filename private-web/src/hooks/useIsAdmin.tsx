import { type ReactNode, createContext, useContext } from "react";
import { useApi } from "../api";

/** `true` if confirmed admin, `false` if confirmed non-admin, `undefined` while
 * the probe is in-flight or errored. Treat undefined as non-admin for gating
 * edit-only UI — better to flicker missing buttons on for an admin than to
 * briefly expose them to a non-admin. */
const AdminContext = createContext<boolean | undefined>(undefined);

/** Single source of truth for the caller's admin status. Mount once near the
 * router root so every consumer reads the same value without each one
 * re-fetching the probe. */
export function AdminProvider({ children }: { children: ReactNode }) {
	const probe = useApi("commons", "is_current_user_admin");
	const value = probe.status === "ok" ? probe.data : undefined;
	return <AdminContext.Provider value={value}>{children}</AdminContext.Provider>;
}

/** Returns `true`/`false` once the probe has resolved, or `undefined` while
 * loading. Callers should `=== true` to gate admin-only UI. */
export function useIsAdmin(): boolean | undefined {
	return useContext(AdminContext);
}
