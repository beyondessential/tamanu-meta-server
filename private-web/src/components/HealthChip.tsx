import { Chip, Tooltip } from "@mui/material";
import CancelIcon from "@mui/icons-material/Cancel";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import type { HealthState } from "../types";

const LABEL: Record<HealthState, string> = {
	healthy: "Healthy",
	warning: "Warning",
	unhealthy: "Unhealthy",
};

const TOOLTIP: Record<HealthState, string> = {
	healthy: "Server reports OK",
	warning: "Some check is warning, failing, or broken while the server reports overall OK",
	unhealthy: "Server reports problems",
};

const COLOR: Record<HealthState, "success" | "warning" | "error"> = {
	healthy: "success",
	warning: "warning",
	unhealthy: "error",
};

/** Headline health chip derived from the `HealthState` rollup (top-level
 * self-report AND per-check results), not the raw top-level `healthy`
 * bool — a server saying "healthy" while a check fails shows as Warning,
 * matching the status-dot border and `<HealthLegend>`.
 *
 * `stale` recolours to an outlined variant for servers that aren't
 * currently reporting, so an operator isn't misled into thinking a stale
 * "Healthy" still holds. */
export default function HealthChip({
	health,
	stale = false,
}: {
	health: HealthState;
	stale?: boolean;
}) {
	if (stale) {
		return (
			<Tooltip title="Server isn't currently reporting status; this reflects its most recent received report.">
				<Chip
					size="small"
					variant="outlined"
					color="warning"
					icon={<WarningAmberIcon />}
					label={`Last reported ${LABEL[health].toLowerCase()}`}
				/>
			</Tooltip>
		);
	}
	const icon =
		health === "healthy" ? (
			<CheckCircleIcon />
		) : health === "warning" ? (
			<WarningAmberIcon />
		) : (
			<CancelIcon />
		);
	return (
		<Tooltip title={TOOLTIP[health]}>
			<Chip size="small" color={COLOR[health]} icon={icon} label={LABEL[health]} />
		</Tooltip>
	);
}
