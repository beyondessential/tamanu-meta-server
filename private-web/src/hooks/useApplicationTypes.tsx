import { type ReactNode, createContext, useContext, useMemo } from "react";
import { useApi } from "../api";
import {
	applicationTypeLabel,
	type ApplicationType,
	type ApplicationTypeInfo,
	type Caps,
	type VersionTracking,
} from "../types";

// `null` (not `undefined`) so consumers can tell "no provider mounted" apart
// from "provider mounted, not yet loaded".
const ApplicationTypesContext = createContext<
	ApplicationTypeInfo[] | null | undefined
>(null);

/** Loads the application-type catalogue once per session and shares it with
 * the tree.
 *
 * What canopy does for a type — whether its applications have a version, and
 * whether that version can be graded — is decided in the backend's capability
 * table. The UI reads it from there rather than keeping its own copy, which
 * would drift the moment a type is added. */
export function ApplicationTypesProvider({
	children,
}: {
	children: ReactNode;
}) {
	// The catalogue is compiled into the backend, so it can't change under a
	// running session: fetch once, no reload interval.
	const probe = useApi("commons", "products", {});
	const types = probe.status === "ok" ? probe.data : undefined;
	return (
		<ApplicationTypesContext.Provider value={types}>
			{children}
		</ApplicationTypesContext.Provider>
	);
}

function useCatalogue(): ApplicationTypeInfo[] | undefined {
	const ctx = useContext(ApplicationTypesContext);
	if (ctx === null) {
		throw new Error(
			"useApplicationTypes must be used inside <ApplicationTypesProvider>",
		);
	}
	return ctx;
}

/** Every type, for pickers. Empty until the catalogue loads. */
export function useApplicationTypes(): ApplicationTypeInfo[] {
	return useCatalogue() ?? [];
}

/** How a type is written in the UI. Falls back to deriving the label the same
 * way the backend does until the catalogue loads, so a chip carries the type
 * rather than an empty space and doesn't change wording once it arrives. */
export function useApplicationTypeLabel(type: ApplicationType): string {
	const catalogue = useCatalogue();
	return useMemo(
		() =>
			catalogue?.find((t) => t.type === type)?.label ??
			applicationTypeLabel(type),
		[catalogue, type],
	);
}

/** One type's capabilities, or `undefined` until the catalogue loads.
 *
 * Callers render nothing rather than guessing while this is `undefined`: a
 * version cell that assumes "tracked" would flash an "unknown" for a type
 * that has no version at all. */
export function useApplicationTypeCaps(
	type: ApplicationType,
): Caps | undefined {
	const catalogue = useCatalogue();
	return useMemo(
		() => catalogue?.find((t) => t.type === type)?.caps,
		[catalogue, type],
	);
}

/** How versions should be treated across a set of applications' types — for a
 * figure that summarises several at once, like a group's headline version.
 *
 * The strongest treatment present wins: a group with one Tamanu member has a
 * version to grade whatever else it holds, and only a group where nothing has
 * a version at all shows none. `undefined` until the catalogue loads. */
export function useVersionTrackingAcross(
	types: readonly ApplicationType[],
): VersionTracking | undefined {
	const catalogue = useCatalogue();
	return useMemo(() => {
		if (!catalogue) return undefined;
		const tracking = types.map(
			(t) => catalogue.find((c) => c.type === t)?.caps.version_tracking,
		);
		if (tracking.includes("tracked")) return "tracked";
		if (tracking.includes("reported")) return "reported";
		return "absent";
	}, [catalogue, types]);
}

/** A predicate for whether a type's version is graded against a release train
 * canopy holds — for filtering a fleet-wide spread down to the applications a
 * version figure actually covers.
 *
 * Reports `true` for every type until the catalogue loads, so the spread
 * starts from what it has always shown and narrows once the answer arrives,
 * rather than briefly dropping rows. */
export function useIsVersionTracked(): (type: ApplicationType) => boolean {
	const catalogue = useCatalogue();
	return useMemo(
		() => (type: ApplicationType) => {
			const found = catalogue?.find((t) => t.type === type);
			return found === undefined || found.caps.version_tracking === "tracked";
		},
		[catalogue],
	);
}
