import { Box, Tooltip } from "@mui/material";
import type { HealthState, ShortStatus } from "../types";

const STATUS_COLOR: Record<ShortStatus, string> = {
	up: "success.main",
	down: "error.main",
	away: "warning.main",
	blip: "secondary.main",
	gone: "text.disabled",
};

// Only meaningful when the server is reachable. When unreachable, the
// fill alone carries the state (and we have no health signal anyway —
// the server isn't talking to us).
const HEALTH_OUTLINE: Record<HealthState, string | null> = {
	healthy: null,
	warning: "warning.main",
	unhealthy: "error.main",
};

const REACHABLE: ReadonlySet<ShortStatus> = new Set(["up", "blip"]);

interface StatusDotProps {
	up: ShortStatus;
	/** Server's self-reported health from its most recent status push.
	 * When supplied and the server is reachable, the dot renders an
	 * outline whose colour encodes the health state. */
	health?: HealthState;
	title?: string;
	dim?: boolean;
	/** Size relative to the surrounding font size. Defaults to "1em". */
	size?: string;
}

export default function StatusDot({
	up,
	health,
	title,
	dim,
	size = "1em",
}: StatusDotProps) {
	const outlineColor =
		health && REACHABLE.has(up) ? HEALTH_OUTLINE[health] : null;
	const dot = (
		<Box
			component="span"
			sx={{
				display: "inline-block",
				width: size,
				height: size,
				borderRadius: "50%",
				bgcolor: STATUS_COLOR[up],
				opacity: dim ? 0.5 : 1,
				marginRight: "0.5em",
				verticalAlign: "middle",
				// Outline rather than border: it draws *outside* the box so
				// adjacent dots don't reflow when a server gains or loses a
				// health ring.
				outline: outlineColor ? "0.25em solid" : "none",
				outlineColor,
				outlineOffset: outlineColor ? "-0.20em" : 0,
			}}
		/>
	);
	if (title) {
		return <Tooltip title={title}>{dot}</Tooltip>;
	}
	return dot;
}
