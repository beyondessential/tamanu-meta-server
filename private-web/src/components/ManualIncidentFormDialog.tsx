import {
	Alert,
	Button,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	MenuItem,
	Stack,
	TextField,
} from "@mui/material";
import { useEffect, useState } from "react";
import { useApi, useApiAction } from "../api";
import type { ManualIncidentData } from "../types";

/** RFC 3339 timestamp → the local-time string a datetime-local input wants. */
function toLocalInput(iso: string): string {
	const d = new Date(iso);
	const pad = (n: number) => String(n).padStart(2, "0");
	return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(
		d.getHours(),
	)}:${pad(d.getMinutes())}`;
}

/**
 * Create or edit a manual incident (a support-recorded record; see the
 * read-only cards on /incidents). Without `incident` it records a new one;
 * with it, it edits that record in place. The affected group is mandatory.
 */
export default function ManualIncidentFormDialog({
	open,
	onClose,
	onSaved,
	incident,
}: {
	open: boolean;
	onClose: () => void;
	onSaved?: () => void;
	incident?: ManualIncidentData;
}) {
	const editing = incident != null;
	const createAction = useApiAction("manual_incidents", "create");
	const updateAction = useApiAction("manual_incidents", "update");
	const action = editing ? updateAction : createAction;
	const groups = useApi("server_groups", "list", {}, []);

	const [title, setTitle] = useState("");
	const [description, setDescription] = useState("");
	const [groupId, setGroupId] = useState("");
	const [startedAt, setStartedAt] = useState("");
	const [endedAt, setEndedAt] = useState("");

	// (Re)fill the form whenever the dialog opens: from the record when
	// editing, or fresh (started now, ongoing) when recording a new one.
	useEffect(() => {
		if (!open) return;
		setTitle(incident?.title ?? "");
		setDescription(incident?.description ?? "");
		setGroupId(incident?.server_group_id ?? "");
		setStartedAt(toLocalInput(incident?.started_at ?? new Date().toISOString()));
		setEndedAt(incident?.ended_at ? toLocalInput(incident.ended_at) : "");
	}, [open, incident]);

	const close = () => {
		action.reset();
		onClose();
	};

	const onSave = async () => {
		try {
			const started = new Date(startedAt).toISOString();
			const ended = endedAt === "" ? null : new Date(endedAt).toISOString();
			if (editing) {
				await updateAction.call({
					id: incident.id,
					title: title.trim(),
					description,
					startedAt: started,
					// The end time can be set, changed, or cleared (ongoing again).
					...(ended === null
						? incident.ended_at != null && { clearEndedAt: true }
						: { endedAt: ended }),
					serverGroupId: groupId,
				});
			} else {
				await createAction.call({
					title: title.trim(),
					description,
					startedAt: started,
					endedAt: ended,
					serverGroupId: groupId,
				});
			}
			onSaved?.();
			close();
		} catch {
			/* surfaced via action.error */
		}
	};

	const incomplete = title.trim() === "" || groupId === "" || startedAt === "";

	return (
		<Dialog open={open} onClose={close} fullWidth maxWidth="sm">
			<DialogTitle>
				{editing ? "Edit manual incident" : "Record manual incident"}
			</DialogTitle>
			<DialogContent>
				<Stack spacing={2} sx={{ mt: 1 }}>
					<TextField
						label="Title"
						value={title}
						onChange={(e) => setTitle(e.target.value)}
						disabled={action.pending}
						required
					/>
					<TextField
						select
						label="Affected group"
						value={groupId}
						onChange={(e) => setGroupId(e.target.value)}
						disabled={action.pending}
						required
					>
						{groups.status === "ok" &&
							groups.data.map((g) => (
								<MenuItem key={g.id} value={g.id}>
									{g.name}
								</MenuItem>
							))}
					</TextField>
					<Stack direction="row" spacing={2}>
						<TextField
							label="Started"
							type="datetime-local"
							value={startedAt}
							onChange={(e) => setStartedAt(e.target.value)}
							disabled={action.pending}
							required
							fullWidth
							slotProps={{ inputLabel: { shrink: true } }}
						/>
						<TextField
							label="Ended (empty while ongoing)"
							type="datetime-local"
							value={endedAt}
							onChange={(e) => setEndedAt(e.target.value)}
							disabled={action.pending}
							fullWidth
							slotProps={{ inputLabel: { shrink: true } }}
						/>
					</Stack>
					<TextField
						label="Description (markdown)"
						multiline
						minRows={4}
						value={description}
						onChange={(e) => setDescription(e.target.value)}
						disabled={action.pending}
					/>
					{action.error && (
						<Alert severity="error">{action.error.message}</Alert>
					)}
				</Stack>
			</DialogContent>
			<DialogActions>
				<Button onClick={close} disabled={action.pending}>
					Cancel
				</Button>
				<Button
					variant="contained"
					onClick={onSave}
					disabled={action.pending || incomplete}
				>
					{action.pending ? "Saving…" : editing ? "Save" : "Record"}
				</Button>
			</DialogActions>
		</Dialog>
	);
}
