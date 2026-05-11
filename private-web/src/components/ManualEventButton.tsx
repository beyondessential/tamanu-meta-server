import { Button, Dialog, DialogContent, DialogTitle } from "@mui/material";
import AddAlertIcon from "@mui/icons-material/AddAlert";
import { useState } from "react";
import { useApi } from "../api";
import ManualEventForm from "./ManualEventForm";
import type { IncidentData } from "../types";

/** Admin-only button on the ServerDetail header: opens a dialog with the
 * manual-event form. Label switches between "New incident" and "Add issue"
 * depending on whether the server already has an open incident.
 *
 * Note: incidents are grouped at the root of the server tree, so on child
 * servers this query returns nothing and the label will read "New incident"
 * even when the root's group has an open one. Operators usually manage
 * incidents from the root page where the Incidents section is shown. */
export default function ManualEventButton({
	serverId,
	onSubmitted,
}: {
	serverId: string;
	/** Called after a successful submission so the parent can refresh sibling
	 * panels (issues, incidents) that are now stale. */
	onSubmitted?: () => void;
}) {
	const [open, setOpen] = useState(false);
	const incidents = useApi<IncidentData[]>(
		"incidents",
		"list_for_server",
		{ server_id: serverId, include_closed: false },
		[serverId],
	);
	const hasOpenIncident =
		incidents.status === "ok" && incidents.data.length > 0;
	const label = hasOpenIncident ? "Add issue" : "New incident";

	return (
		<>
			<Button
				variant="outlined"
				startIcon={<AddAlertIcon />}
				onClick={() => setOpen(true)}
			>
				{label}
			</Button>
			<Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="sm">
				<DialogTitle>{label}</DialogTitle>
				<DialogContent>
					<ManualEventForm
						serverId={serverId}
						onSubmitted={() => {
							setOpen(false);
							incidents.reload();
							onSubmitted?.();
						}}
					/>
				</DialogContent>
			</Dialog>
		</>
	);
}
