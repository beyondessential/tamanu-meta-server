import OpenInNewIcon from "@mui/icons-material/OpenInNew";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import ActionButton from "./ActionButton";
import { useApi } from "../api";
import { useIsNotificationHeld } from "../hooks/useIsNotificationHeld";
import { isIncidentLingering } from "../types";

/** Server-detail header button into the incidents view. Any server id in
 * a group works — the backend resolves to the group root. Three states:
 * - open incident exists → direct link to /incidents/:id (error-coloured;
 *   warning-coloured when the Slack notice is still inside the per-group
 *   cooldown window so the operator can tell at a glance that nobody else
 *   has been paged yet; info-coloured when the incident is lingering —
 *   every failure has recovered and it closes if things stay quiet)
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

	const heldUntilActive = useIsNotificationHeld(
		openIncident?.notification_held_until ?? null,
	);
	if (openIncident) {
		const lingering = isIncidentLingering(openIncident);
		const held = heldUntilActive && !lingering;
		return (
			<ActionButton
				to={`/incidents/${openIncident.id}`}
				color={lingering ? "info" : held ? "warning" : "error"}
				icon={<WarningAmberIcon />}
				title={
					lingering
						? "All failures have recovered; the incident closes if they stay quiet through the linger window"
						: held
							? "Slack notice still inside the per-group cooldown window"
							: undefined
				}
				label={`Incident ${openIncident.id.slice(0, 8)}${
					lingering ? " (recovering)" : held ? " (held)" : ""
				}`}
			/>
		);
	}
	if (hasActive) {
		return (
			<ActionButton
				to={`/incidents?group=${serverId}`}
				icon={<OpenInNewIcon />}
				label="Active issues"
			/>
		);
	}
	return (
		<ActionButton
			to={`/incidents?group=${serverId}&showAll=1`}
			icon={<OpenInNewIcon />}
			label="Past issues"
		/>
	);
}
