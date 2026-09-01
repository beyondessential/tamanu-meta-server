import { Box, Tooltip } from "@mui/material";
import type { HealthState, ShortStatus } from "../types";

// One dot, one subject, one colourway.
//
// The dot used to say two things at once — a fill for reachability and a ring
// for health — because a server was a box and the software on it at once. The
// machine owns reachability now and carries it on the enclosure around the dot
// (see `MachineEnclosure`), so the dot spends its whole colourway on the one
// application, and an operator reads severity from the colour rather than
// decoding a ring.
//
// Red appears on both grains deliberately: red means down, and which element
// carries it says what went down. A red dot in a neutral pill is one
// application failing on a healthy box; a red pill is the box.
// spec: CHK#presentation
const HEALTHY = "success.main";
// A near neighbour of the healthy green, on purpose. A warning is "could be
// better" rather than "degraded", so it reads as a shade of fine rather than a
// shade of trouble, and does not compete with a red for attention. Yellow stays
// out of the ramp entirely, being spoken for by the striped check-broken state.
const WARNING = "rgb(132, 223, 91)";
const DOWN = "error.main";
const NEVER = "text.disabled";

// Diagonal cut for unmonitored targets. A mask punches the band out of the
// dot rather than painting over it, so the surface behind shows through
// whatever it is — these dots sit on plain paper, on operator-tinted group
// cards, and on hover backgrounds.
const UNMONITORED_MASK =
	"linear-gradient(135deg, #000 0 42%, transparent 42% 58%, #000 58% 100%)";

// A window's cut runs the other way, so a target being worked on is not read
// as one nobody is watching at all.
const MAINTAINED_MASK =
	"linear-gradient(45deg, #000 0 42%, transparent 42% 58%, #000 58% 100%)";

/// What colour this target's dot takes.
///
/// Never reported wins over everything: there is nothing to say about a target
/// that has never spoken. Otherwise silence and self-reported failure are both
/// red — the application is not serving either way, and which it is reads from
/// the tooltip and its own page rather than from a second colour.
export function dotColor(up: ShortStatus, health?: HealthState): string {
	if (up === "gone") return NEVER;
	if (up === "down" || health === "unhealthy") return DOWN;
	if (health === "warning") return WARNING;
	return HEALTHY;
}

interface StatusDotProps {
	up: ShortStatus;
	/** The target's health from its current check state. Folded into the one
	 * colourway with reachability rather than carried as a separate ring. */
	health?: HealthState;
	title?: string;
	dim?: boolean;
	/** Size relative to the surrounding font size. Defaults to "1em". */
	size?: string;
	/** Whether canopy alerts on this target. Unmonitored targets are cut
	 * through with a diagonal so a red dot doesn't read as "someone is
	 * being paged about this" — their checks are determined and shown as
	 * normal but raise nothing. Defaults to monitored. */
	// spec: CHK#monitoring-gate
	monitored?: boolean;
	/** Whether a maintenance window suspends this target. Cut the other way
	 * from unmonitored: the work is deliberate and temporary, and its checks
	 * are still recorded and shown. Defaults to not under maintenance. */
	// spec: MNT#presentation
	maintained?: boolean;
}

export default function StatusDot({
	up,
	health,
	title,
	dim,
	size = "1em",
	monitored = true,
	maintained = false,
}: StatusDotProps) {
	const mask = maintained
		? MAINTAINED_MASK
		: monitored
			? undefined
			: UNMONITORED_MASK;
	const dot = (
		<Box
			component="span"
			sx={{
				display: "inline-block",
				width: size,
				height: size,
				borderRadius: "50%",
				bgcolor: dotColor(up, health),
				opacity: dim ? 0.5 : 1,
				marginRight: "0.5em",
				verticalAlign: "middle",
				maskImage: mask,
				WebkitMaskImage: mask,
			}}
		/>
	);
	const tooltip = [
		title,
		maintained ? "under maintenance" : null,
		monitored ? null : "unmonitored",
	]
		.filter(Boolean)
		.join(" · ");
	if (tooltip) {
		return <Tooltip title={tooltip}>{dot}</Tooltip>;
	}
	return dot;
}
