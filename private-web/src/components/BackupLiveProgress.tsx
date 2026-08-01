import { LinearProgress, Stack, Tooltip, Typography } from "@mui/material";

import { formatBytes } from "../lib/formatBytes";
import { humanSeconds } from "../lib/humanDuration";
import type { LiveProgress } from "../types";

/// Compact live figures for a backup in flight: what it has moved against what it
/// expects, its rate, and how long since the device was last heard from.
///
/// Sized for an inline row (a server's capability list) rather than a table cell.
/// Renders nothing when the run reports no progress — an older client's row keeps
/// its "backing up…" chip and gains nothing here, rather than showing zeroes.
export function BackupLiveProgress({
	progress,
}: {
	progress: LiveProgress | null | undefined;
}) {
	if (!progress) return null;
	const moved = progress.bytes_uploaded;
	const expected = progress.bytes_estimated;
	const pct =
		moved != null && expected != null && expected > 0
			? Math.min(100, (moved / expected) * 100)
			: null;
	// Nothing worth a line: no volume and no rate. The chip already says it's
	// running.
	if (moved == null && progress.bytes_per_second == null) return null;
	return (
		<Stack spacing={0.25} sx={{ maxWidth: 260 }} data-testid="capability-progress">
			<Typography variant="caption" color="text.secondary">
				{moved == null ? "—" : formatBytes(moved)}
				{expected != null && ` / ~${formatBytes(expected)}`}
				{progress.bytes_per_second != null &&
					` · ${formatBytes(progress.bytes_per_second)}/s`}
			</Typography>
			{pct != null && (
				<Tooltip
					title={`${Math.round(pct)}% of the run's own estimate, which it may revise upward`}
				>
					<LinearProgress
						variant="determinate"
						value={pct}
						sx={{ height: 4, borderRadius: 2 }}
					/>
				</Tooltip>
			)}
			<Typography variant="caption" color="text.secondary">
				last heard {humanSeconds(progress.seconds_since_observed)} ago
			</Typography>
		</Stack>
	);
}
