import { Tooltip } from "@mui/material";
import { useEffect, useState } from "react";

function formatSecs(secs: number): string {
	const s = Math.abs(Math.floor(secs));
	if (s < 3600) return `${Math.floor(s / 60)}m`;
	if (s < 86_400) return `${Math.floor(s / 3600)}h`;
	return `${Math.floor(s / 86_400)}d`;
}

/** Renders a relative time like "5m ago" with the absolute timestamp as a tooltip.
 * Refreshes every 10 seconds while the page is visible. */
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

	const text = `${formatSecs((now - ts) / 1000)} ago`;
	return (
		<Tooltip title={new Date(ts).toLocaleString()}>
			<span>{text}</span>
		</Tooltip>
	);
}
