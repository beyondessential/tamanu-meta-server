import { Box, Chip, Stack, Tooltip } from "@mui/material";
import CancelIcon from "@mui/icons-material/Cancel";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import BuildOutlinedIcon from "@mui/icons-material/BuildOutlined";
import NotificationsOffIcon from "@mui/icons-material/NotificationsOff";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import type { HealthState } from "../types";

const UNMONITORED_TOOLTIP =
	"This server is unmonitored — its checks are recorded and shown, but nothing alerts on them.";

const MAINTAINED_TOOLTIP =
	"A maintenance window is being worked on here — its checks are recorded and shown, and raise nothing until the window ends.";

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
 * "Healthy" still holds.
 *
 * `monitored={false}` fades the chip and sets a silence icon beside it:
 * canopy determines an unmonitored server's health the same way it does
 * everyone else's, so it can read Unhealthy while nobody is being paged.
 * The chip keeps its real colour and label underneath — the state is
 * true, it's the alerting that's off. */
// spec: CHK#monitoring-gate
export default function HealthChip({
	health,
	stale = false,
	monitored = true,
	maintained = false,
}: {
	health: HealthState;
	stale?: boolean;
	monitored?: boolean;
	/** Whether a maintenance window suspends the server: the chip fades and
	 * takes a maintenance marker, distinct from the unmonitored one. */
	// spec: MNT#presentation
	maintained?: boolean;
}) {
	const chip = stale ? (
		<Tooltip title="Server isn't currently reporting status; this reflects its most recent received report.">
			<Chip
				size="small"
				variant="outlined"
				color="warning"
				icon={<WarningAmberIcon />}
				label={`Last reported ${LABEL[health].toLowerCase()}`}
			/>
		</Tooltip>
	) : (
		<Tooltip title={TOOLTIP[health]}>
			<Chip
				size="small"
				color={COLOR[health]}
				icon={
					health === "healthy" ? (
						<CheckCircleIcon />
					) : health === "warning" ? (
						<WarningAmberIcon />
					) : (
						<CancelIcon />
					)
				}
				label={LABEL[health]}
			/>
		</Tooltip>
	);
	if (monitored && !maintained) return chip;
	return (
		<Stack direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
			<Box sx={{ opacity: 0.5, display: "inline-flex" }}>{chip}</Box>
			{maintained && (
				<Tooltip title={MAINTAINED_TOOLTIP}>
					<BuildOutlinedIcon
						fontSize="small"
						color="info"
						data-testid="maintenance-marker"
					/>
				</Tooltip>
			)}
			{!monitored && (
				<Tooltip title={UNMONITORED_TOOLTIP}>
					<NotificationsOffIcon
						fontSize="small"
						color="info"
						data-testid="unmonitored-marker"
					/>
				</Tooltip>
			)}
		</Stack>
	);
}
