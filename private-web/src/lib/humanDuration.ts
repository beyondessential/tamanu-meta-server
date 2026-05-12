/** Coarse, single-unit duration: "12s", "3m", "5h", "2d". */
export function humanDuration(earliestIso: string, latestIso: string): string {
	const ms = Date.parse(latestIso) - Date.parse(earliestIso);
	if (ms <= 0) return "0s";
	const sec = Math.round(ms / 1000);
	if (sec < 60) return `${sec}s`;
	const min = Math.round(sec / 60);
	if (min < 60) return `${min}m`;
	const hr = Math.round(min / 60);
	if (hr < 24) return `${hr}h`;
	const day = Math.round(hr / 24);
	return `${day}d`;
}
