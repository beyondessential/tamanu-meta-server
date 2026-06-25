import { Chip, CircularProgress, Tooltip } from "@mui/material";

import TimeAgo from "./TimeAgo";

/// Shown when a backup of a given type looks in flight: credentials were issued
/// recently and no run has been reported since. `since` is the issuance time;
/// `null`/absent renders nothing.
export function BackupProcessingChip({
	since,
}: {
	since: string | null | undefined;
}) {
	if (!since) return null;
	return (
		<Tooltip
			title={
				<>
					Credentials issued <TimeAgo timestamp={since} />; awaiting the run
					report.
				</>
			}
		>
			<Chip
				size="small"
				color="info"
				variant="outlined"
				icon={<CircularProgress size={12} color="inherit" />}
				label="backing up…"
			/>
		</Tooltip>
	);
}
