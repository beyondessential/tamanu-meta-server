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
	children,
}: {
	up: ShortStatus;
	health: HealthState;
	/** The box's name, for the tooltip. */
	name?: string | null;
	/** The dots for the applications on this machine. */
	children: ReactNode;
}) {
	const title = [name, enclosureTitle(up, health)].filter(Boolean).join(" — ");
	return (
		<Tooltip title={title}>
			<Box
				component="span"
				sx={{
					display: "inline-flex",
					alignItems: "center",
					bgcolor: enclosureColor(up, health),
					borderRadius: "999px",
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
