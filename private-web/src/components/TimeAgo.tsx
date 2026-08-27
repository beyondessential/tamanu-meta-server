import { Tooltip } from "@mui/material";
import { useEffect, useState } from "react";

function formatSecs(secs: number): string {
	const s = Math.abs(secs);
	const minutes = Math.round(s / 60);
	if (minutes < 60) return `${minutes}m`;
	const hours = Math.round(s / 3600);
	if (hours < 24) return `${hours}h`;
	return `${Math.round(s / 86_400)}d`;
}

/** Renders a relative time like "5m ago" — or "in 5m" when the timestamp is
 * in the future. With the absolute timestamp as a tooltip. Refreshes every
 * 10 seconds while the page is visible. */
export default function TimeAgo({ timestamp }: { timestamp: string }) {
	const ts = Date.parse(timestamp);
	const [now, setNow] = useState(() => Date.now());

	useEffect(() => {
		if (Number.isNaN(ts)) return;
		const id = window.setInterval(() => {
			if (!document.hidden) setNow(Date.now());
		}, 10_000);
		return () => window.clearInterval(id);
	}, [ts]);

	if (Number.isNaN(ts)) return <span>?</span>;

	const secs = (now - ts) / 1000;
	const text =
		Math.abs(secs) < 60
			? "now"
			: secs < 0
				? `in ${formatSecs(secs)}`
				: `${formatSecs(secs)} ago`;
	return (
		<Tooltip title={new Date(ts).toLocaleString()}>
			<span>{text}</span>
		</Tooltip>
	);
}
