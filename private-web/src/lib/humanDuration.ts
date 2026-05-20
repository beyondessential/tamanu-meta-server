/** Coarse, single-unit duration: "12s", "3m", "5h", "2d". */
export function humanDuration(earliestIso: string, latestIso: string): string {
	const ms = Date.parse(latestIso) - Date.parse(earliestIso);
	return humanSeconds(Math.round(ms / 1000));
}

export function humanSeconds(seconds: number): string {
	if (seconds <= 0) return "0s";
	if (seconds < 60) return `${seconds}s`;
	const min = Math.round(seconds / 60);
	if (min < 60) return `${min}m`;
	const hr = Math.round(min / 60);
	if (hr < 24) return `${hr}h`;
	const day = Math.round(hr / 24);
	return `${day}d`;
}
