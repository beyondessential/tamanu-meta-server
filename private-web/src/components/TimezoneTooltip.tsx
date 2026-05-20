import { Tooltip } from "@mui/material";
import { useEffect, useMemo, useState } from "react";

function formatLocalTime(tz: string, now: Date): string | null {
	try {
		return new Intl.DateTimeFormat(undefined, {
			timeZone: tz,
			weekday: "short",
			day: "numeric",
			month: "short",
			hour: "2-digit",
			minute: "2-digit",
		}).format(now);
	} catch {
		return null;
	}
}

/** Renders an IANA timezone name with the current local time at that
 * timezone in a tooltip. Falls back to plain text if the timezone is
 * unrecognised by the browser. Refreshes every minute while visible. */
export default function TimezoneTooltip({ tz }: { tz: string }) {
	const valid = useMemo(
		() => formatLocalTime(tz, new Date()) !== null,
		[tz],
	);
	if (!valid) return <>{tz}</>;
	return (
		<Tooltip title={<LocalTime tz={tz} />}>
			<span style={{ cursor: "help" }}>{tz}</span>
		</Tooltip>
	);
}

function LocalTime({ tz }: { tz: string }) {
	const [now, setNow] = useState(() => new Date());
	useEffect(() => {
		const id = window.setInterval(() => {
			if (!document.hidden) setNow(new Date());
		}, 60_000);
		return () => window.clearInterval(id);
	}, []);
	return <>Local time: {formatLocalTime(tz, now) ?? "—"}</>;
}
