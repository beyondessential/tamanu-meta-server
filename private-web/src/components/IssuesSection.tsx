import {
	Alert as MuiAlert,
	FormControlLabel,
	IconButton,
	LinearProgress,
	Paper,
	Stack,
	Switch,
	Typography,
} from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";
import { useState } from "react";
import { useApi } from "../api";
import IssueRow from "./IssueRow";

export default function IssuesSection({
	scope,
	id,
	refreshKey = 0,
	onChanged,
}: {
	scope: "device" | "server";
	id: string;
	/** Bump to force a refetch (e.g. after a sibling component submits an event). */
	refreshKey?: number;
	/** Called on any mutation. Lets the parent refresh sibling panels.
	 * Falls back to local reload when unset. */
	onChanged?: () => void;
}) {
	const [showAll, setShowAll] = useState(false);
	const result = useApi(
		"issues",
		scope === "device" ? "list_for_device" : "list_for_server",
		scope === "device"
			? { device_id: id, active_only: !showAll }
			: { application_id: id, active_only: !showAll },
		[scope, id, showAll, refreshKey],
	);
	const notify = onChanged ?? result.reload;

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="h6" component="h3">
					Issues
					{result.status === "ok" && result.data.length > 0
						? ` (${result.data.length})`
						: ""}
				</Typography>
				<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
					<FormControlLabel
						control={
							<Switch
								size="small"
								checked={showAll}
								onChange={(e) => setShowAll(e.target.checked)}
							/>
						}
						label="Show all"
					/>
					<IconButton aria-label="Refresh issues" size="small" onClick={notify}>
						<RefreshIcon fontSize="small" />
					</IconButton>
				</Stack>
			</Stack>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<MuiAlert severity="error">{result.error.message}</MuiAlert>
			) : result.data.length === 0 ? (
				<MuiAlert severity="success">No {showAll ? "" : "active "}issues.</MuiAlert>
			) : (
				<Stack spacing={1}>
					{result.data.map((i) => (
						<IssueRow key={i.id} issue={i} onChanged={notify} />
					))}
				</Stack>
			)}
		</Paper>
	);
}
