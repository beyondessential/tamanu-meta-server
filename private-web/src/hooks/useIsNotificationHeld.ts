import { useEffect, useState } from "react";

/** "Held" means the per-group Slack cooldown hasn't elapsed yet — so the
 * incident is open but operators haven't been paged. Once `heldUntil`
 * slips into the past, the worker has either shipped the notice or is
 * about to; the UI can't be sure without a refetch, so the safest signal
 * is "no longer held" and the page will catch up on its next reload.
 *
 * Ticks every 10s while visible so badges that key off this hide
 * themselves automatically when the deadline passes — without that, a
 * stale page would keep claiming "held" indefinitely. */
export function useIsNotificationHeld(heldUntil: string | null): boolean {
	const ts = heldUntil ? Date.parse(heldUntil) : NaN;
	const [now, setNow] = useState(() => Date.now());
	useEffect(() => {
		if (Number.isNaN(ts)) return;
		const id = window.setInterval(() => {
			if (!document.hidden) setNow(Date.now());
		}, 10_000);
		return () => window.clearInterval(id);
	}, [ts]);
	if (Number.isNaN(ts)) return false;
	return ts > now;
}
