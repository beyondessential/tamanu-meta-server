import { Stack, Typography } from "@mui/material";
import type { ShortStatus } from "../types";
import StatusDot from "./StatusDot";
import VersionSquare from "./VersionSquare";

const STATUS_ENTRIES: Array<{ up: ShortStatus; label: string }> = [
	{ up: "up", label: "Up (seen a minute ago)" },
	{ up: "blip", label: "Blip (missed 2 checks)" },
	{ up: "away", label: "Away (last seen 2-10m ago)" },
	{ up: "down", label: "Down (last seen 10m-7d ago)" },
	{ up: "gone", label: "Gone (never or more than 7d ago)" },
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
		</Stack>
	);
}
