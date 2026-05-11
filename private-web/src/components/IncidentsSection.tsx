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
import IncidentRow from "./IncidentRow";
import type { IncidentData } from "../types";

export default function IncidentsSection({
	serverId,
	refreshKey = 0,
	onChanged,
}: {
	serverId: string;
	/** Bump to force a refetch. */
	refreshKey?: number;
	/** Called on any mutation. Falls back to local reload when unset. */
	onChanged?: () => void;
}) {
	const [showAll, setShowAll] = useState(false);
	const result = useApi<IncidentData[]>(
		"incidents",
		"list_for_server",
		{ server_id: serverId, include_closed: showAll },
		[serverId, showAll, refreshKey],
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
					Incidents
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
					<IconButton aria-label="Refresh incidents" size="small" onClick={notify}>
						<RefreshIcon fontSize="small" />
					</IconButton>
				</Stack>
			</Stack>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<MuiAlert severity="error">{result.error.message}</MuiAlert>
			) : result.data.length === 0 ? (
				<MuiAlert severity="success">
					No {showAll ? "" : "open "}incidents.
				</MuiAlert>
			) : (
				<Stack spacing={1}>
					{result.data.map((inc) => (
						<IncidentRow key={inc.id} incident={inc} onChanged={notify} />
					))}
				</Stack>
			)}
		</Paper>
	);
}
