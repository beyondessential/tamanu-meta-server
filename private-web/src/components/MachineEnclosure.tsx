import { Box, Tooltip } from "@mui/material";
import type { ReactNode } from "react";
import type { HealthState, ShortStatus } from "../types";

// The pill is the machine, so it carries the machine's state while the dots
// inside carry each application's. Every machine is enclosed, whether it runs
// one application or several: an enclosure never means anything on its own,
// only its contents do. A box hosting two workloads is one pill with two dots,
// which is the whole point of the grain.
//
// The pill is drawn as an outline with a wash rather than a solid fill, so the
// dots inside stay the loudest thing in it. A machine's state is context for
// its applications', not a competitor to it.
//
// Orange is the enclosure's alone, so each hue means one thing: light green a
// degraded application, orange a degraded machine, red down. A red pill is the
// box, and everything on it is unreachable with it.
// spec: CHK#presentation
const STATES = {
	fine: { border: "rgba(0, 0, 0, 0.18)", fill: "transparent" },
	degraded: { border: "warning.main", fill: "rgba(237, 108, 2, 0.10)" },
	down: { border: "error.main", fill: "rgba(211, 47, 47, 0.12)" },
	// A box that has never reported: outlined like any other, washed out rather
	// than coloured, since there is nothing yet to say about it.
	never: { border: "rgba(0, 0, 0, 0.12)", fill: "action.disabledBackground" },
} as const;

// A window hatches the pill's wash rather than cutting through it the way a
// dot's does. A mask on the enclosure would clip the dots inside it as well,
// which would say something about the applications — and a window is the box's.
// The hatch runs the same way as the dot's maintenance cut, so the two read as
// one idea at either grain.
// spec: MNT#presentation
const MAINTAINED_HATCH =
	"repeating-linear-gradient(45deg, transparent 0 4px, rgba(0, 0, 0, 0.16) 4px 8px)";

function enclosureState(up: ShortStatus, health: HealthState) {
	if (up === "gone") return STATES.never;
	if (up === "down") return STATES.down;
	if (health === "unhealthy" || health === "warning") return STATES.degraded;
	return STATES.fine;
}

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
	const state = enclosureState(up, health);
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
					gap: "0.35em",
					border: 1,
					borderColor: state.border,
					bgcolor: state.fill,
					backgroundImage: maintained ? MAINTAINED_HATCH : undefined,
					borderRadius: "999px",
					px: "4px",
					py: "3px",
					// The dots carry their own right margin, which the pill's own
					// gap replaces.
					"& span": { marginRight: 0 },
				}}
			>
				{children}
			</Box>
		</Tooltip>
	);
}
