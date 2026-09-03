import OpenInNewIcon from "@mui/icons-material/OpenInNew";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import ActionButton from "./ActionButton";
import { useApi } from "../api";
import { useIsNotificationHeld } from "../hooks/useIsNotificationHeld";
import { isIncidentLingering } from "../types";

/** Server-detail header button into the incidents view. Incidents are looked
 * up by server (the backend resolves to the group root); issues are looked up
 * by the server's group, which the caller must supply — `issues.list` matches
 * `serverGroupId` against `servers.group_id` and does no server→group
 * resolution of its own, so a server id there matches nothing at all. Three
 * states:
 * - open incident exists → direct link to /incidents/:id (error-coloured;
 *   warning-coloured when the Slack notice is still inside the per-group
 *   cooldown window so the operator can tell at a glance that nobody else
 *   has been paged yet; info-coloured when the incident is lingering —
 *   every failure has recovered and it closes if things stay quiet)
 * - no open incident, but active issues → /incidents filtered to this group
 * - nothing active → same, with `showAll=1` so closed/inactive surface
 *
 * An ungrouped server has no group to filter by, so the issues query is
 * skipped and the links fall back to the unfiltered incidents view. */
export default function IncidentsLink({
	applicationId = null,
	groupId,
	refreshKey = 0,
}: {
	/// The application this button sits on, where it sits on one. A machine
	/// page passes none: an incident belongs to the group either way, and a
	/// box has no per-application lookup to go through.
	applicationId?: string | null;
	groupId: string | null;
	refreshKey?: number;
}) {
	const byApplication = useApi(
		"incidents",
		"list_for_server",
		{ server_id: applicationId ?? "", include_closed: false },
		[applicationId, refreshKey],
		{ skip: applicationId === null },
	);
	const byGroup = useApi(
		"incidents",
		"list_for_group",
		{ server_group_id: groupId ?? "", include_closed: false, limit: 1 },
		[groupId, refreshKey],
		{ skip: applicationId !== null || groupId === null },
	);
	const incidents = applicationId !== null ? byApplication : byGroup;
	const openIncident =
		incidents.status === "ok" && incidents.data.length > 0
			? incidents.data[0]
			: null;

	const issues = useApi(
		"issues",
		"list",
		{ activeOnly: true, serverGroupId: groupId ?? undefined, limit: 1 },
		[groupId, refreshKey],
		{ skip: groupId === null },
	);
	const hasActive = issues.status === "ok" && issues.data.length > 0;

	const incidentsHref = (showAll: boolean) => {
		const params = new URLSearchParams();
		if (groupId !== null) params.set("group", groupId);
		if (showAll) params.set("showAll", "1");
		const query = params.toString();
		return query ? `/incidents?${query}` : "/incidents";
	};

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
				to={incidentsHref(false)}
				icon={<OpenInNewIcon />}
				label="Active issues"
			/>
		);
	}
	return (
		<ActionButton
			to={incidentsHref(true)}
			icon={<OpenInNewIcon />}
			label="Past issues"
		/>
	);
}
