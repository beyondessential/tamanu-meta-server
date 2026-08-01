/** Coarse, single-unit duration: "12s", "3m", "5h", "2d". */
export function humanDuration(earliestIso: string, latestIso: string): string {
	const ms = Date.parse(latestIso) - Date.parse(earliestIso);
	return humanSeconds(Math.round(ms / 1000));
}

export function humanSeconds(seconds: number): string {
	if (seconds <= 0) return "0s";
	if (seconds < 60) return `${seconds}s`;
	// Each unit is computed from the raw seconds, never from the previous
	// *rounded* unit — compounding the roundings inflated durations by nearly
	// a whole unit at the top of a range (5375s, i.e. 1h29m35s, rounded to
	// 90m and then to "2h").
	//
	// The rounded lower unit still chooses when to step up, so a duration
	// that rounds to 60 minutes reads "1h" rather than "60m".
	const min = Math.round(seconds / 60);
	if (min < 60) return `${min}m`;
	const hr = Math.round(seconds / 3600);
	if (hr < 24) return `${hr}h`;
	return `${Math.round(seconds / 86400)}d`;
}
