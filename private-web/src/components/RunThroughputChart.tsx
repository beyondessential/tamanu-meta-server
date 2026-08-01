import { Box, Paper, Typography, useTheme } from "@mui/material";
import { useMemo, useState } from "react";

import { formatBytes } from "../lib/formatBytes";
import { humanSeconds } from "../lib/humanDuration";
import type { RunProgressPoint } from "../types";

/// Upload rate over a run's life, from the progress series it reported.
///
/// One measure, one axis. Cumulative volume is deliberately *not* plotted here:
/// bytes and bytes-per-second don't share a scale, so putting both on one frame
/// would make their crossing point an artifact of the axis choice rather than
/// anything about the run. Volume is carried by the in-flight row's progress bar.
///
/// The series colour is stepped per surface rather than flipped: the theme's dark
/// primary is too light to read as a mark against a dark background, so each mode
/// takes its own step from the same green ramp.
const SERIES = { light: "#388e3c", dark: "#43a047" };

const HEIGHT = 132;
const PAD = { top: 18, right: 12, bottom: 22, left: 40 };

/// One unit for the whole axis, chosen from its upper bound.
///
/// Ticks are bare numbers and the unit is stated once, because `formatBytes`
/// picks a unit per value — so per-tick formatting could label one tick in MiB and
/// the next in GiB, which is unreadable on an axis. It also keeps the labels
/// narrow enough not to need a wide gutter.
function rateUnit(max: number): { label: string; divisor: number } {
	const units: [string, number][] = [
		["B/s", 1],
		["KiB/s", 1024],
		["MiB/s", 1024 ** 2],
		["GiB/s", 1024 ** 3],
	];
	let chosen = units[0];
	for (const unit of units) {
		if (max >= unit[1]) chosen = unit;
	}
	return { label: chosen[0], divisor: chosen[1] };
}

type RatePoint = { t: number; elapsed: number; rate: number };

/// Differences of the cumulative counters. Each rate is plotted at the *later* of
/// the two samples it came from — the interval it describes ends there.
///
/// A sample missing `bytes_uploaded` is skipped rather than read as zero, and a
/// negative difference is dropped: counters are cumulative and monotonic, so a
/// decrease means a restarted or misreporting client, not negative throughput.
export function toRatePoints(series: RunProgressPoint[]): RatePoint[] {
	const usable = series.filter((p) => p.bytes_uploaded != null);
	if (usable.length < 2) return [];
	const start = Date.parse(usable[0].observed_at);
	const out: RatePoint[] = [];
	for (let i = 1; i < usable.length; i++) {
		const t0 = Date.parse(usable[i - 1].observed_at);
		const t1 = Date.parse(usable[i].observed_at);
		const seconds = (t1 - t0) / 1000;
		const moved = (usable[i].bytes_uploaded ?? 0) - (usable[i - 1].bytes_uploaded ?? 0);
		if (seconds <= 0 || moved < 0) continue;
		out.push({ t: t1, elapsed: (t1 - start) / 1000, rate: moved / seconds });
	}
	return out;
}

/// Clean-ish upper bound and three ticks, so the axis reads in round numbers
/// rather than at the data's exact maximum.
function niceMax(max: number): number {
	if (max <= 0) return 1;
	const pow = 10 ** Math.floor(Math.log10(max));
	const scaled = max / pow;
	const step = scaled <= 1 ? 1 : scaled <= 2 ? 2 : scaled <= 5 ? 5 : 10;
	return step * pow;
}

export default function RunThroughputChart({
	series,
	width = 520,
}: {
	series: RunProgressPoint[];
	width?: number;
}) {
	const theme = useTheme();
	const dark = theme.palette.mode === "dark";
	const stroke = dark ? SERIES.dark : SERIES.light;
	const surface = theme.palette.background.paper;
	const grid = theme.palette.divider;
	const [hover, setHover] = useState<number | null>(null);

	const points = useMemo(() => toRatePoints(series), [series]);

	if (points.length === 0) {
		return (
			<Typography variant="body2" color="text.secondary" data-testid="throughput-empty">
				Not enough progress reported to chart a rate yet.
			</Typography>
		);
	}

	const plotW = width - PAD.left - PAD.right;
	const plotH = HEIGHT - PAD.top - PAD.bottom;
	const maxElapsed = points[points.length - 1].elapsed || 1;
	const maxRate = niceMax(Math.max(...points.map((p) => p.rate)));

	const x = (elapsed: number) => PAD.left + (elapsed / maxElapsed) * plotW;
	const y = (rate: number) => PAD.top + plotH - (rate / maxRate) * plotH;

	const line = points.map((p) => `${x(p.elapsed)},${y(p.rate)}`).join(" ");
	const area = `${PAD.left},${PAD.top + plotH} ${line} ${x(maxElapsed)},${PAD.top + plotH}`;
	const ticks = [0, 0.5, 1].map((f) => f * maxRate);
	const unit = rateUnit(maxRate);
	const tickLabel = (t: number) => {
		const v = t / unit.divisor;
		return v === 0 ? "0" : v >= 10 ? v.toFixed(0) : v.toFixed(1);
	};
	const last = points[points.length - 1];
	const shown = hover != null ? points[hover] : null;

	/// Snap the crosshair to the nearest point by x — readers aim at a time, not
	/// at a 2px line.
	const onMove = (event: React.PointerEvent<SVGSVGElement>) => {
		const rect = event.currentTarget.getBoundingClientRect();
		const px = ((event.clientX - rect.left) / rect.width) * width;
		let best = 0;
		let bestDist = Infinity;
		points.forEach((p, i) => {
			const d = Math.abs(x(p.elapsed) - px);
			if (d < bestDist) {
				bestDist = d;
				best = i;
			}
		});
		setHover(best);
	};

	return (
		<Box sx={{ position: "relative", maxWidth: width }} data-testid="throughput-chart">
			<svg
				viewBox={`0 0 ${width} ${HEIGHT}`}
				width="100%"
				height={HEIGHT}
				role="img"
				aria-label={`Upload rate over the run, peaking at ${formatBytes(maxRate)} per second`}
				onPointerMove={onMove}
				onPointerLeave={() => setHover(null)}
				style={{ display: "block", touchAction: "none" }}
			>
				{ticks.map((t) => (
					<g key={t}>
						<line
							x1={PAD.left}
							x2={PAD.left + plotW}
							y1={y(t)}
							y2={y(t)}
							stroke={grid}
							strokeWidth={1}
						/>
						<text
							x={PAD.left - 6}
							y={y(t) + 3.5}
							textAnchor="end"
							fontSize={10}
							fill={theme.palette.text.secondary}
						>
							{tickLabel(t)}
						</text>
					</g>
				))}

				{/* The axis unit, stated once. */}
				<text
					x={0}
					y={10}
					fontSize={10}
					fill={theme.palette.text.secondary}
				>
					{unit.label}
				</text>

				<polygon points={area} fill={stroke} fillOpacity={0.1} />
				<polyline
					points={line}
					fill="none"
					stroke={stroke}
					strokeWidth={2}
					strokeLinejoin="round"
					strokeLinecap="round"
				/>

				{/* End marker carries a surface ring so it stays legible over the line. */}
				<circle
					cx={x(last.elapsed)}
					cy={y(last.rate)}
					r={4}
					fill={stroke}
					stroke={surface}
					strokeWidth={2}
				/>

				{shown && (
					<>
						<line
							x1={x(shown.elapsed)}
							x2={x(shown.elapsed)}
							y1={PAD.top}
							y2={PAD.top + plotH}
							stroke={theme.palette.text.disabled}
							strokeWidth={1}
						/>
						<circle
							cx={x(shown.elapsed)}
							cy={y(shown.rate)}
							r={4}
							fill={stroke}
							stroke={surface}
							strokeWidth={2}
						/>
					</>
				)}

				<line
					x1={PAD.left}
					x2={PAD.left}
					y1={PAD.top}
					y2={PAD.top + plotH}
					stroke={grid}
					strokeWidth={1}
				/>
				<text
					x={PAD.left}
					y={HEIGHT - 6}
					fontSize={10}
					fill={theme.palette.text.secondary}
				>
					start
				</text>
				<text
					x={PAD.left + plotW}
					y={HEIGHT - 6}
					textAnchor="end"
					fontSize={10}
					fill={theme.palette.text.secondary}
				>
					{humanSeconds(Math.round(maxElapsed))}
				</text>
			</svg>

			{shown && (
				<Paper
					variant="outlined"
					sx={{
						position: "absolute",
						top: 0,
						left: Math.min(x(shown.elapsed) + 8, width - 140),
						px: 1,
						py: 0.5,
						pointerEvents: "none",
						minWidth: 120,
					}}
				>
					{/* Value leads, label follows: the reader already has the series. */}
					<Typography variant="body2" sx={{ fontWeight: 600 }}>
						{formatBytes(shown.rate)}/s
					</Typography>
					<Typography variant="caption" color="text.secondary">
						{humanSeconds(Math.round(shown.elapsed))} in
					</Typography>
				</Paper>
			)}
		</Box>
	);
}
