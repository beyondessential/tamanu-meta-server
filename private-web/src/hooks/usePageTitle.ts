import { useEffect } from "react";

const APP_NAME = "Canopy";

/** Sets `document.title` to "<page> · Canopy" while this component is mounted.
 * Pass `null` or an empty string to render just "Canopy". */
export function usePageTitle(page: string | null | undefined): void {
	useEffect(() => {
		const title = page ? `${page} · ${APP_NAME}` : APP_NAME;
		const previous = document.title;
		document.title = title;
		return () => {
			document.title = previous;
		};
	}, [page]);
}
