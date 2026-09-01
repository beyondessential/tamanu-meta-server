import { Chip, Stack, Typography } from "@mui/material";
import PersonIcon from "@mui/icons-material/Person";
import type { HealthState, ShortStatus } from "../types";
import MachineEnclosure from "./MachineEnclosure";
import StatusDot from "./StatusDot";
import VersionSquare from "./VersionSquare";

// One colourway, so one legend: the dot's colour is the application's state,
// and the pill around it is the box's. No durations here — each target is
// judged against its own threshold, so a fixed one would be wrong for most of
// the fleet.
// spec: CHK#reachability
const DOT_ENTRIES: Array<{
	up: ShortStatus;
	health: HealthState;
	label: string;
}> = [
	{ up: "up", health: "healthy", label: "Healthy (application reports OK)" },
	{
		up: "up",
		health: "warning",
		label: "Warning (some check failing, overall OK)",
	},
	{ up: "up", health: "unhealthy", label: "Failing (application reports problems)" },
	{ up: "down", health: "healthy", label: "Down (silent past its threshold)" },
	{ up: "gone", health: "healthy", label: "Never reported" },
];

// The enclosure's own states. Orange is the pill's alone, so each hue means
// one thing: light green a degraded application, orange a degraded machine,
// red down.
const MACHINE_ENTRIES: Array<{
	up: ShortStatus;
	health: HealthState;
	label: string;
}> = [
	{ up: "up", health: "healthy", label: "Machine fine" },
	{ up: "up", health: "warning", label: "Machine's own checks degraded" },
	{ up: "down", health: "healthy", label: "Machine down (everything on it with it)" },
];

const VERSION_ENTRIES: Array<{ distance: number | null; label: string }> = [
	{ distance: 1, label: "Up to date" },
	{ distance: 3, label: "2-4 versions behind" },
	{ distance: 7, label: "5-9 versions behind" },
	{ distance: 11, label: "10+ versions behind" },
	{ distance: null, label: "Version not known" },
];

export function VersionLegend() {
	return (
		<Stack direction="row" spacing={2} useFlexGap sx={{ flexWrap: "wrap" }}>
			{VERSION_ENTRIES.map(({ distance, label }) => (
				<Stack key={label} direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
					<VersionSquare distance={distance} />
					<Typography variant="body2" color="text.secondary">
						{label}
					</Typography>
				</Stack>
			))}
		</Stack>
	);
}

export function StatusLegend() {
	return (
		<Stack direction="row" spacing={2} useFlexGap sx={{ flexWrap: "wrap" }}>
			{DOT_ENTRIES.map(({ up, health, label }) => (
				<Stack
					key={label}
					direction="row"
					spacing={0.5}
					sx={{ alignItems: "center" }}
				>
					<StatusDot up={up} health={health} />
					<Typography variant="body2" color="text.secondary">
						{label}
					</Typography>
				</Stack>
			))}
			<Stack direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
				<StatusDot up="down" monitored={false} />
				<Typography variant="body2" color="text.secondary">
					Cut through: unmonitored (state shown, nothing alerts)
				</Typography>
			</Stack>
			<Stack direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
				<StatusDot up="down" maintained />
				<Typography variant="body2" color="text.secondary">
					Cut the other way: under maintenance (being worked on)
				</Typography>
			</Stack>
		</Stack>
	);
}

export function OperatorLegend() {
	return (
		<Stack direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
			<Chip icon={<PersonIcon />} label="2" size="small" variant="outlined" />
			<Typography variant="body2" color="text.secondary">
				Operators connected over Tailscale right now (card also tinted)
			</Typography>
		</Stack>
	);
}

/// The pill around a group card's dots: the box, whose state is not the state
/// of the software on it.
export function HealthLegend() {
	return (
		<Stack direction="row" spacing={2} useFlexGap sx={{ flexWrap: "wrap" }}>
			{MACHINE_ENTRIES.map(({ up, health, label }) => (
				<Stack
					key={label}
					direction="row"
					spacing={0.5}
					sx={{ alignItems: "center" }}
				>
					<MachineEnclosure up={up} health={health}>
						<StatusDot up="up" health="healthy" />
					</MachineEnclosure>
					<Typography variant="body2" color="text.secondary">
						{label}
					</Typography>
				</Stack>
			))}
		</Stack>
	);
}
