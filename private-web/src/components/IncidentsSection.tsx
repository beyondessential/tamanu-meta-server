import { useApi } from "../api";
import IncidentRow from "./IncidentRow";
import type { IncidentData } from "../types";

/** Renders the (at-most-one) open incident for a server-group inline. There
 * can't be more than one open incident per group, so this is just "the open
 * incident if any" — no Paper wrapper, no list-style heading, no toggle. */
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
	const result = useApi<IncidentData[]>(
		"incidents",
		"list_for_server",
		{ server_id: serverId, include_closed: false },
		[serverId, refreshKey],
	);
	const notify = onChanged ?? result.reload;

	if (result.status !== "ok" || result.data.length === 0) return null;
	const incident = result.data[0];
	return <IncidentRow incident={incident} onChanged={notify} />;
}
