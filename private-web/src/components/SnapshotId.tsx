import CheckIcon from "@mui/icons-material/Check";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import { IconButton, Stack, Tooltip, Typography } from "@mui/material";
import { useState } from "react";

import { formatBytes } from "../lib/formatBytes";
import TimeAgo from "./TimeAgo";

/// A kopia snapshot id shown truncated (they're long and opaque), with a button
/// to copy the full value. Renders "—" when there's no snapshot.
export function SnapshotId({ id }: { id: string | null | undefined }) {
	const [copied, setCopied] = useState(false);
	if (!id) return <>—</>;
	const onCopy = async () => {
		try {
			await navigator.clipboard.writeText(id);
			setCopied(true);
			window.setTimeout(() => setCopied(false), 2000);
		} catch {
			/* clipboard may be unavailable; ignore */
		}
	};
	const short = id.length > 12 ? `${id.slice(0, 12)}…` : id;
	return (
		<Stack
			direction="row"
			spacing={0.5}
			component="span"
			sx={{ alignItems: "center" }}
		>
			<Typography
				component="code"
				variant="body2"
				sx={{ fontFamily: "monospace" }}
				title={id}
			>
				{short}
			</Typography>
			<Tooltip title={copied ? "Copied" : "Copy snapshot ID"}>
				<IconButton size="small" aria-label="Copy snapshot ID" onClick={onCopy}>
					{copied ? (
						<CheckIcon fontSize="inherit" />
					) : (
						<ContentCopyIcon fontSize="inherit" />
					)}
				</IconButton>
			</Tooltip>
		</Stack>
	);
}

/// The latest snapshot for a backup type: its id (copyable), when it was taken,
/// and how much it uploaded. Renders an explicit "no snapshot yet" when the
/// type has never produced a successful backup.
export function LatestSnapshot({
	id,
	at,
	bytes,
}: {
	id: string | null | undefined;
	at: string | null | undefined;
	bytes: number | null | undefined;
}) {
	if (!id && !at) {
		return (
			<Typography variant="caption" color="text.secondary">
				no snapshot yet
			</Typography>
		);
	}
	return (
		<Stack
			direction="row"
			spacing={1}
			useFlexGap
			sx={{ alignItems: "center", flexWrap: "wrap" }}
		>
			<SnapshotId id={id} />
			{at && (
				<Typography variant="caption" color="text.secondary">
					<TimeAgo timestamp={at} />
				</Typography>
			)}
			{bytes != null && (
				<Typography variant="caption" color="text.secondary">
					{formatBytes(bytes)}
				</Typography>
			)}
		</Stack>
	);
}
