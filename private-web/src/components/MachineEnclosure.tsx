import { Box, Tooltip } from "@mui/material";
import type { ReactNode } from "react";
import type { HealthState, ShortStatus } from "../types";

// The pill is the machine, so it carries the machine's state while the dots
// inside carry each application's. Every machine is enclosed, whether it runs
// one application or several: an enclosure never means anything on its own,
// only its contents do. A box hosting two workloads is one pill with two dots,
// which is the whole point of the grain.
//
// Orange is the enclosure's alone, so each hue means one thing: light green a
// degraded application, orange a degraded machine, red down. A red pill is the
// box, and everything on it is unreachable with it.
// spec: CHK#presentation
const NEUTRAL = "action.hover";
const DEGRADED = "warning.light";
const DOWN = "error.light";
const NEVER = "action.disabledBackground";

function enclosureColor(up: ShortStatus, health: HealthState): string {
	if (up === "gone") return NEVER;
	if (up === "down") return DOWN;
	if (health === "unhealthy" || health === "warning") return DEGRADED;
	return NEUTRAL;
}

// A window hatches the pill's fill rather than cutting through it the way a
// dot's does. A mask on the enclosure would clip the dots inside it as well,
// which would say something about the applications — and a window is the box's.
// The hatch runs the same way as the dot's maintenance cut, so the two read as
// one idea at either grain.
// spec: MNT#presentation
const MAINTAINED_HATCH =
	"repeating-linear-gradient(45deg, transparent 0 4px, rgba(0, 0, 0, 0.16) 4px 8px)";

function enclosureTitle(up: ShortStatus, health: HealthState): string {
	if (up === "gone") return "Machine has never reported";
	if (up === "down") return "Machine unreachable";
	if (health === "unhealthy") return "Machine's own checks failing";
	if (health === "warning") return "Machine's own checks warning";
	return "Machine healthy";
}

export default function MachineEnclosure({
	up,
	health,
	name,
	maintained = false,
	children,
}: {
	up: ShortStatus;
	health: HealthState;
	/** The box's name, for the tooltip. */
	name?: string | null;
	/** Whether a maintenance window suspends this box, its own or its group's.
	 * A window is declared over a machine and never over an application, so it
	 * is the enclosure that carries it — the applications inside are suspended
	 * by their box rather than each saying so. */
	// spec: MNT#presentation
	maintained?: boolean;
	/** The dots for the applications on this machine. */
	children: ReactNode;
}) {
	const title = [
		name,
		enclosureTitle(up, health),
		maintained ? "under maintenance" : null,
	]
		.filter(Boolean)
		.join(" — ");
	return (
		<Tooltip title={title}>
			<Box
				component="span"
				sx={{
					display: "inline-flex",
					alignItems: "center",
					bgcolor: enclosureColor(up, health),
					borderRadius: "999px",
					backgroundImage: maintained ? MAINTAINED_HATCH : undefined,
					px: 0.5,
					py: 0.25,
					// The dots carry their own right margin; trim the last one so
					// the pill closes evenly around whatever it holds.
					"& > *:last-child": { marginRight: 0 },
				}}
			>
				{children}
			</Box>
		</Tooltip>
	);
}
