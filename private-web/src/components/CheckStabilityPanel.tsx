import { Box, Stack, Tooltip, Typography, alpha, useTheme } from "@mui/material";
import TimeAgo from "./TimeAgo";
import { humanSeconds } from "../lib/humanDuration";
import type { StabilityData } from "../types";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// A check state's stability record: the derived flap statistics in a
/// sentence, and the hour-of-week degradation profile as a 7×24 heatmap.
/// Magnitude (share of observations degraded) is a sequential ramp of one
/// hue over the surface; hours with no observations render hollow.
export default function CheckStabilityPanel({
	stability,
}: {
	stability: StabilityData;
}) {
	const { stats } = stability;
	return (
		<Stack spacing={1}>
			<Typography variant="body2" color="text.secondary">
				Observed {stability.observations} time
				{stability.observations === 1 ? "" : "s"} (
				{stability.degraded_observations} degraded).{" "}
				{stats.flips_24h > 0 || stats.flips_7d > 0 ? (
					<>
						{stats.flips_24h} state change
						{stats.flips_24h === 1 ? "" : "s"} in 24 h, {stats.flips_7d} in 7
						days
						{stats.ring_covers_from && (
							<>
								{" "}
								(history since <TimeAgo timestamp={stats.ring_covers_from} />)
							</>
						)}
						.
					</>
				) : (
					<>No state changes in the last 7 days.</>
				)}
				{stats.typical_degraded_run_secs != null && (
					<>
						{" "}
						Typical degraded run{" "}
						{humanSeconds(stats.typical_degraded_run_secs)}
						{stats.typical_healthy_gap_secs != null && (
							<>
								, typical healthy gap{" "}
								{humanSeconds(stats.typical_healthy_gap_secs)}
							</>
						)}
						.
					</>
				)}
			</Typography>
			<DutyHeatmap cells={stability.duty_cycle} />
			<HeatmapCaption />
		</Stack>
	);
}

/// Fleet-level rollup of a check's stability across every server on the
/// page: summed duty profiles into one heatmap, plus aggregate flap
/// counts. Renders nothing when no server has a record yet.
export function FleetStabilitySummary({
	records,
}: {
	records: StabilityData[];
}) {
	if (records.length === 0) return null;
	const flips24 = records.reduce((n, r) => n + r.stats.flips_24h, 0);
	const flips7d = records.reduce((n, r) => n + r.stats.flips_7d, 0);
	const flappedServers = records.filter((r) => r.stats.flips_7d > 0).length;
	const cells = Array.from({ length: 168 }, (_, i) => ({
		observations: records.reduce(
			(n, r) => n + (r.duty_cycle[i]?.observations ?? 0),
			0,
		),
		degraded: records.reduce((n, r) => n + (r.duty_cycle[i]?.degraded ?? 0), 0),
	}));
	return (
		<Stack spacing={1}>
			<Typography variant="body2" color="text.secondary">
				Across {records.length} server{records.length === 1 ? "" : "s"} with a
				record:{" "}
				{flips7d > 0 ? (
					<>
						{flips24} state change{flips24 === 1 ? "" : "s"} in 24 h, {flips7d}{" "}
						in 7 days, on {flappedServers} server
						{flappedServers === 1 ? "" : "s"}.
					</>
				) : (
					<>no state changes in the last 7 days.</>
				)}
			</Typography>
			<DutyHeatmap cells={cells} />
			<HeatmapCaption />
		</Stack>
	);
}

export function HeatmapCaption() {
	return (
		<Typography variant="caption" color="text.secondary">
			Share of observations degraded per hour of week (UTC), leaning towards
			recent weeks. Hollow cells have no observations.
		</Typography>
	);
}

/// The 7×24 hour-of-week heatmap on its own, for reuse by fleet-level
/// rollups. Renders nothing without a full, non-empty profile.
export function DutyHeatmap({
	cells,
}: {
	cells: StabilityData["duty_cycle"];
}) {
	const theme = useTheme();
	if (cells.length !== 168 || cells.every((c) => c.observations === 0)) {
		return null;
	}
	return (
		<Box sx={{ overflowX: "auto" }}>
			<Box
				sx={{
					display: "grid",
					gridTemplateColumns: "max-content repeat(24, 12px)",
					gridAutoRows: "12px",
					gap: "2px",
					alignItems: "center",
					width: "max-content",
				}}
			>
				{DAYS.map((day, d) => (
					<Row key={day} day={day} row={d} cells={cells} theme={theme} />
				))}
				{/* Hour ticks under the grid. */}
				<Box />
				{Array.from({ length: 24 }, (_, h) => (
					<Typography
						key={h}
						variant="caption"
						color="text.secondary"
						sx={{ fontSize: "0.6rem", lineHeight: 1, textAlign: "left" }}
					>
						{h % 6 === 0 ? `${h}` : ""}
					</Typography>
				))}
			</Box>
		</Box>
	);
}

function Row({
	day,
	row,
	cells,
	theme,
}: {
	day: string;
	row: number;
	cells: StabilityData["duty_cycle"];
	theme: ReturnType<typeof useTheme>;
}) {
	return (
		<>
			<Typography
				variant="caption"
				color="text.secondary"
				sx={{ pr: 0.5, lineHeight: 1 }}
			>
				{day}
			</Typography>
			{Array.from({ length: 24 }, (_, hour) => {
				const cell = cells[row * 24 + hour]!;
				const fraction =
					cell.observations > 0 ? cell.degraded / cell.observations : null;
				const label =
					fraction == null
						? `${day} ${hour}:00–${hour + 1}:00 UTC — no observations`
						: `${day} ${hour}:00–${hour + 1}:00 UTC — degraded ${Math.round(
								fraction * 100,
							)}% of ${cell.observations} observation${
								cell.observations === 1 ? "" : "s"
							}`;
				return (
					<Tooltip key={hour} title={label}>
						<Box
							data-testid="duty-cell"
							data-fraction={fraction ?? undefined}
							sx={{
								width: 12,
								height: 12,
								borderRadius: 0.5,
								...(fraction == null
									? { border: 1, borderColor: "divider" }
									: {
											// Sequential single-hue ramp over the surface:
											// a visible floor so 0% still reads as
											// observed-and-healthy, darkening to full
											// error hue at always-degraded.
											bgcolor: alpha(
												theme.palette.error.main,
												0.06 + 0.94 * fraction,
											),
										}),
							}}
						/>
					</Tooltip>
				);
			})}
		</>
	);
}
