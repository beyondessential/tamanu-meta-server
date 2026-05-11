import { Button } from "@mui/material";
import OpenInNewIcon from "@mui/icons-material/OpenInNew";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { Link as RouterLink } from "react-router-dom";
import { useApi } from "../api";
import type { IncidentData, IssueData } from "../types";

/** Server-detail header button into the incidents view. Three states:
 * - open incident exists → direct link to /incidents/:id (error-coloured)
 * - no open incident, but active issues → /incidents filtered to this group
 * - nothing active → same, with `showAll=1` so closed/inactive surface */
export default function IncidentsLink({
	rootServerId,
	refreshKey = 0,
}: {
	rootServerId: string;
	refreshKey?: number;
}) {
	const incidents = useApi<IncidentData[]>(
		"incidents",
		"list_for_server",
		{ server_id: rootServerId, include_closed: false },
		[rootServerId, refreshKey],
	);
	const openIncident =
		incidents.status === "ok" && incidents.data.length > 0
			? incidents.data[0]
			: null;

	const issues = useApi<IssueData[]>(
		"issues",
		"list",
		{ activeOnly: true, serverGroupId: rootServerId, limit: 1 },
		[rootServerId, refreshKey],
	);
	const hasActive = issues.status === "ok" && issues.data.length > 0;

	if (openIncident) {
		return (
			<Button
				component={RouterLink}
				to={`/incidents/${openIncident.id}`}
				variant="outlined"
				color="error"
				size="small"
				startIcon={<WarningAmberIcon />}
			>
				Open incident
			</Button>
		);
	}
	if (hasActive) {
		return (
			<Button
				component={RouterLink}
				to={`/incidents?group=${rootServerId}`}
				variant="outlined"
				size="small"
				startIcon={<OpenInNewIcon />}
			>
				Active issues
			</Button>
		);
	}
	return (
		<Button
			component={RouterLink}
			to={`/incidents?group=${rootServerId}&showAll=1`}
			variant="outlined"
			size="small"
			startIcon={<OpenInNewIcon />}
		>
			Past issues
		</Button>
	);
}
