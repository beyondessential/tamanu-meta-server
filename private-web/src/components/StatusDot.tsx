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
	warning: "rgb(132, 223, 91)",
	unhealthy: "success.main",
};

const REACHABLE: ReadonlySet<ShortStatus> = new Set(["up", "blip"]);

// Diagonal cut for unmonitored servers. A mask punches the band out of the
// dot rather than painting over it, so the surface behind shows through
// whatever it is — these dots sit on plain paper, on operator-tinted group
// cards, and on hover backgrounds.
const UNMONITORED_MASK =
	"linear-gradient(135deg, #000 0 42%, transparent 42% 58%, #000 58% 100%)";

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
	/** Whether canopy alerts on this server. Unmonitored servers are cut
	 * through with a diagonal so a red dot doesn't read as "someone is
	 * being paged about this" — their checks are determined and shown as
	 * normal but raise nothing. Defaults to monitored. */
	// spec: CHK#monitoring-gate
	monitored?: boolean;
}

export default function StatusDot({
	up,
	health,
	title,
	dim,
	size = "1em",
	monitored = true,
}: StatusDotProps) {
	const outlineColor =
		health && REACHABLE.has(up) ? HEALTH_OUTLINE[health] : null;
	// Errors swap the dot's fill and outline colours instead of stacking a
	// red ring on top of the green "up" fill.
	const bgColor =
		health === "unhealthy" && REACHABLE.has(up)
			? "error.main"
			: STATUS_COLOR[up];
	const dot = (
		<Box
			component="span"
			sx={{
				display: "inline-block",
				width: size,
				height: size,
				borderRadius: "50%",
				bgcolor: bgColor,
				opacity: dim ? 0.5 : 1,
				marginRight: "0.5em",
				verticalAlign: "middle",
				// Outline rather than border: it draws *outside* the box so
				// adjacent dots don't reflow when a server gains or loses a
				// health ring.
				outline: outlineColor ? "0.2em solid" : "none",
				outlineColor,
				outlineOffset: outlineColor ? "-0.2em" : 0,
				maskImage: monitored ? undefined : UNMONITORED_MASK,
				WebkitMaskImage: monitored ? undefined : UNMONITORED_MASK,
			}}
		/>
	);
	const tooltip = monitored
		? title
		: [title, "unmonitored"].filter(Boolean).join(" · ");
	if (tooltip) {
		return <Tooltip title={tooltip}>{dot}</Tooltip>;
	}
	return dot;
}
