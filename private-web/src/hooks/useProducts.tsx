import { type ReactNode, createContext, useContext, useMemo } from "react";
import { useApi } from "../api";
import type {
	Caps,
	Product,
	ProductInfo,
	ServerKind,
	VersionTracking,
} from "../types";

// `null` (not `undefined`) so consumers can tell "no provider mounted" apart
// from "provider mounted, not yet loaded".
const ProductsContext = createContext<ProductInfo[] | null | undefined>(null);

/** Loads the product catalogue once per session and shares it with the tree.
 *
 * What canopy does for a product — whether its servers have a version, whether
 * that version can be graded, which roles it defines — is decided in the
 * backend's capability table. The UI reads it from there rather than keeping
 * its own copy, which would drift the moment a product is added. */
export function ProductsProvider({ children }: { children: ReactNode }) {
	// The catalogue is compiled into the backend, so it can't change under a
	// running session: fetch once, no reload interval.
	const probe = useApi("commons", "products", {});
	const products = probe.status === "ok" ? probe.data : undefined;
	return (
		<ProductsContext.Provider value={products}>
			{children}
		</ProductsContext.Provider>
	);
}

function useProductCatalogue(): ProductInfo[] | undefined {
	const ctx = useContext(ProductsContext);
	if (ctx === null) {
		throw new Error("useProducts must be used inside <ProductsProvider>");
	}
	return ctx;
}

/** Every product, for pickers. Empty until the catalogue loads. */
export function useProducts(): ProductInfo[] {
	return useProductCatalogue() ?? [];
}

/** One product's capabilities, or `undefined` until the catalogue loads.
 *
 * Callers render nothing rather than guessing while this is `undefined`: a
 * version cell that assumes "tracked" would flash an "unknown" for a product
 * that has no version at all. */
export function useProductCaps(product: Product): Caps | undefined {
	const catalogue = useProductCatalogue();
	return useMemo(
		() => catalogue?.find((p) => p.product === product)?.caps,
		[catalogue, product],
	);
}

/** How versions should be treated across a set of servers' products — for a
 * figure that summarises several servers at once, like a group's headline
 * version.
 *
 * The strongest treatment present wins: a group with one Tamanu member has a
 * version to grade whatever else it holds, and only a group where nothing has
 * a version at all shows none. `undefined` until the catalogue loads. */
export function useVersionTrackingAcross(
	products: readonly Product[],
): VersionTracking | undefined {
	const catalogue = useProductCatalogue();
	return useMemo(() => {
		if (!catalogue) return undefined;
		const tracking = products.map(
			(p) => catalogue.find((c) => c.product === p)?.caps.version_tracking,
		);
		if (tracking.includes("tracked")) return "tracked";
		if (tracking.includes("reported")) return "reported";
		return "absent";
	}, [catalogue, products]);
}

/** A predicate for whether a product's version is graded against a release
 * train canopy holds — for filtering a fleet-wide spread down to the servers a
 * version figure actually covers.
 *
 * Reports `true` for every product until the catalogue loads, so the spread
 * starts from what it has always shown and narrows once the answer arrives,
 * rather than briefly dropping rows. */
export function useIsVersionTracked(): (product: Product) => boolean {
	const catalogue = useProductCatalogue();
	return useMemo(
		() => (product: Product) => {
			const found = catalogue?.find((p) => p.product === product);
			return found === undefined || found.caps.version_tracking === "tracked";
		},
		[catalogue],
	);
}

/** The roles a product defines, in rank order. Empty until loaded. */
export function useProductKinds(product: Product): ServerKind[] {
	const catalogue = useProductCatalogue();
	return useMemo(
		() => catalogue?.find((p) => p.product === product)?.kinds ?? [],
		[catalogue, product],
	);
}
