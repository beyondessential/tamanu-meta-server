import { Box, Tooltip, keyframes, type Theme } from "@mui/material";
import { alpha } from "@mui/material/styles";
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
	fine: { border: "divider", fill: "transparent" },
	degraded: { border: "warning.main", fill: "rgba(237, 108, 2, 0.10)" },
	down: { border: "error.main", fill: "rgba(211, 47, 47, 0.12)" },
	// A box that has never reported: outlined like any other, washed out rather
	// than coloured, since there is nothing yet to say about it.
	never: { border: "action.disabled", fill: "action.disabledBackground" },
} as const;

// The mark belongs at the grain the operator declared at, so only a window over
// the box itself stripes its icon. The ink is the text colour rather than a
// fixed grey, so the stripes hold on a dark card, and they carry their own
// phase so no background offset exposes the gradient's tile as a seam.
// spec: MNT#presentation
/// A slow drift for stripes at `pitch`, for a window that still holds. Work
/// under way moves and a target serving out the settle period is still, so
/// motion says someone is in there now.
///
/// The travel is one whole tile of the surface's own pitch, measured along the
/// 45 degree gradient, or the loop shows a seam every time it restarts.
// spec: MNT#presentation
export function driftWhileHolding(pitch: number, holding: boolean) {
	if (!holding) return {};
	const travel = keyframes`
		from { background-position: 0 0; }
		to { background-position: ${pitch * Math.SQRT2}px 0; }
	`;
	return {
		animation: `${travel} 3s linear infinite`,
		"@media (prefers-reduced-motion: reduce)": { animation: "none" },
	};
}

export function ownWindowStripes(theme: Theme, settling: boolean): string {
	// The settle period drops much further than the window itself: nobody is in
	// there any more, and the mark is only saying watching has yet to resume.
	const ink = alpha(theme.palette.text.primary, settling ? 0.16 : 0.55);
	return `repeating-linear-gradient(45deg, ${ink} 0 1px, transparent 1px 3px, ${ink} 3px 4px)`;
}

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
	settling = false,
	ownWindow = false,
	describes,
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
	/** Whether every window over the box has ended and it is serving out the
	 * settle period, still suspended but no longer being worked on. */
	// spec: MNT#settling
	settling?: boolean;
	/** Whether the window covering it was declared over this box in
	 * particular. One that reaches it through its environment or its group is
	 * marked at that grain instead, so the icon stays plain. */
	// spec: MNT#presentation
	ownWindow?: boolean;
	/** What the dots inside stand for, one line each. The enclosure names them
	 * so the dots need no tooltip of their own: two tooltips over the same few
	 * pixels open together and overlap, and the reader loses both. */
	describes?: string[];
	/** The dots for the applications on this machine. */
	children: ReactNode;
}) {
	const state = enclosureState(up, health);
	const box = [
		name,
		enclosureTitle(up, health),
		maintained
			? settling
				? "maintenance just ended, watching resumes shortly"
				: "under maintenance"
			: null,
	]
		.filter(Boolean)
		.join(" · ");
	const title = [box, ...(describes ?? [])].join("\n");
	return (
		<Tooltip
			title={title}
			slotProps={{ tooltip: { sx: { whiteSpace: "pre-line" } } }}
		>
			<Box
				component="span"
				// What the icon draws, which is the box's own window. A window
				// reaching it through its environment or its group is marked at
				// that grain, though the tooltip still says the box is suspended.
				// spec: MNT#presentation
				data-maintenance={
					ownWindow ? (settling ? "settling" : "holding") : undefined
				}
				sx={{
					display: "inline-flex",
					alignItems: "center",
					// Everything inside is sized in em, so without a scale of its
					// own a pill takes the font-size of whatever surrounds it and
					// comes out a different size on each surface. One rem here is
					// what makes a pill on a card, in a tree and in the legend the
					// same pill.
					fontSize: "1rem",
					lineHeight: 1,
					gap: "0.35em",
					border: 1,
					borderColor: state.border,
					bgcolor: state.fill,
					backgroundImage: (theme) =>
						ownWindow ? ownWindowStripes(theme, settling) : undefined,
					...driftWhileHolding(4, ownWindow && !settling),
					// Every suspended box is muted, whether the window is its own
					// or reaches it through its environment or its group: all of
					// them are out of play, so a failing one does not read as one
					// nobody has noticed. The stripes stay at the grain the window
					// was declared over; the fade says which boxes it caught.
					// spec: MNT#presentation
					opacity: maintained ? 0.55 : 1,

					borderRadius: "999px",
					// The band keeps its old proportion to the dot inside it: both
					// grew together, so the ring reads as the same ring.
					px: "0.2em",
					py: "0.2em",
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
