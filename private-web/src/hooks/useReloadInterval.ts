import { useEffect, useState } from "react";

/**
 * Returns a tick counter that increments approximately every `intervalMs` while
 * the document is visible, when the document becomes visible after being hidden
 * for longer than the interval, or when the configured custom event fires on
 * the document.
 *
 * Pass the tick into a `useApi` deps array (or any other dependency list) to
 * refetch on each tick.
 */
export function useReloadInterval(
	intervalMs: number,
	customEvent?: string,
): number {
	const [tick, setTick] = useState(0);

	useEffect(() => {
		let lastReload = Date.now();
		const bump = () => {
			lastReload = Date.now();
			setTick((t) => t + 1);
		};

		const intervalId = window.setInterval(() => {
			if (!document.hidden) bump();
		}, intervalMs);

		const onVisibility = () => {
			if (!document.hidden && Date.now() - lastReload > intervalMs) {
				bump();
			}
		};
		document.addEventListener("visibilitychange", onVisibility);

		const onCustom = customEvent ? () => bump() : null;
		if (onCustom && customEvent) {
			document.addEventListener(customEvent, onCustom);
		}

		return () => {
			window.clearInterval(intervalId);
			document.removeEventListener("visibilitychange", onVisibility);
			if (onCustom && customEvent) {
				document.removeEventListener(customEvent, onCustom);
			}
		};
	}, [intervalMs, customEvent]);

	return tick;
}
