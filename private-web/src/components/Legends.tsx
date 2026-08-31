import { Chip, Stack, Typography } from "@mui/material";
import PersonIcon from "@mui/icons-material/Person";
import type { HealthState, ShortStatus } from "../types";
import StatusDot from "./StatusDot";
import VersionSquare from "./VersionSquare";

// No durations here: each target is judged against its own threshold, so a
// fixed one in the legend would be wrong for most of the fleet. The old labels
// named fixed bands and had already drifted from the code.
// spec: CHK#reachability
const STATUS_ENTRIES: Array<{ up: ShortStatus; label: string }> = [
	{ up: "up", label: "Up (reporting)" },
	{ up: "down", label: "Down (silent past its threshold)" },
	{ up: "gone", label: "Gone (never reported)" },
];

const HEALTH_ENTRIES: Array<{ health: HealthState; label: string }> = [
	{ health: "healthy", label: "Healthy (server reports OK)" },
	{ health: "warning", label: "Warning (some check failing, overall OK)" },
	{ health: "unhealthy", label: "Unhealthy (server reports problems)" },
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
			{STATUS_ENTRIES.map(({ up, label }) => (
				<Stack key={up} direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
					<StatusDot up={up} />
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

export function HealthLegend() {
	return (
		<Stack direction="row" spacing={2} useFlexGap sx={{ flexWrap: "wrap" }}>
			{HEALTH_ENTRIES.map(({ health, label }) => (
				<Stack
					key={health}
					direction="row"
					spacing={0.5}
					sx={{ alignItems: "center" }}
				>
					<StatusDot up="up" health={health} />
					<Typography variant="body2" color="text.secondary">
						{label}
					</Typography>
				</Stack>
			))}
		</Stack>
	);
}
