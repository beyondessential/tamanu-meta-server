import { Button } from "@mui/material";
import OpenInNewIcon from "@mui/icons-material/OpenInNew";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { Link as RouterLink } from "react-router-dom";
import { useApi } from "../api";
import { useIsNotificationHeld } from "../hooks/useIsNotificationHeld";

/** Server-detail header button into the incidents view. Any server id in
 * a group works — the backend resolves to the group root. Three states:
 * - open incident exists → direct link to /incidents/:id (error-coloured,
 *   or warning-coloured when the Slack notice is still inside the
 *   per-group cooldown window so the operator can tell at a glance that
 *   nobody else has been paged yet)
 * - no open incident, but active issues → /incidents filtered to this group
 * - nothing active → same, with `showAll=1` so closed/inactive surface */
export default function IncidentsLink({
	serverId,
	refreshKey = 0,
}: {
	serverId: string;
	refreshKey?: number;
}) {
	const incidents = useApi(
		"incidents",
		"list_for_server",
		{ server_id: serverId, include_closed: false },
		[serverId, refreshKey],
	);
	const openIncident =
		incidents.status === "ok" && incidents.data.length > 0
			? incidents.data[0]
			: null;

	const issues = useApi(
		"issues",
		"list",
		{ activeOnly: true, serverGroupId: serverId, limit: 1 },
		[serverId, refreshKey],
	);
	const hasActive = issues.status === "ok" && issues.data.length > 0;

	const held = useIsNotificationHeld(
		openIncident?.notification_held_until ?? null,
	);
	if (openIncident) {
		return (
			<Button
				component={RouterLink}
				to={`/incidents/${openIncident.id}`}
				variant="outlined"
				color={held ? "warning" : "error"}
				startIcon={<WarningAmberIcon />}
				title={
					held
						? "Slack notice still inside the per-group cooldown window"
						: undefined
				}
			>
				Incident {openIncident.id.slice(0, 8)}
				{held && " (held)"}
			</Button>
		);
	}
	if (hasActive) {
		return (
			<Button
				component={RouterLink}
				to={`/incidents?group=${serverId}`}
				variant="outlined"
				startIcon={<OpenInNewIcon />}
			>
				Active issues
			</Button>
		);
	}
	return (
		<Button
			component={RouterLink}
			to={`/incidents?group=${serverId}&showAll=1`}
			variant="outlined"
			startIcon={<OpenInNewIcon />}
		>
			Past issues
		</Button>
	);
}
