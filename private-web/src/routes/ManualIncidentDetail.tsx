import {
	Alert as MuiAlert,
	Button,
	Chip,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Stack,
	Typography,
} from "@mui/material";
import { useState } from "react";
import {
	Link as RouterLink,
	useNavigate,
	useParams,
} from "react-router-dom";
import { useApi, useApiAction } from "../api";
import Markdown from "../components/Markdown";
import ManualIncidentFormDialog from "../components/ManualIncidentFormDialog";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";

/** One manual incident: a support-recorded record, written here or over the
 * MCP interface. Editable in place; deleting returns to the incidents page. */
export default function ManualIncidentDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const navigate = useNavigate();
	const detail = useApi("manual_incidents", "get", { id }, [id]);
	const [editOpen, setEditOpen] = useState(false);
	const [confirmDelete, setConfirmDelete] = useState(false);
	const deleteAction = useApiAction("manual_incidents", "delete");

	usePageTitle(detail.status === "ok" ? detail.data.title : "Manual incident");

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <MuiAlert severity="error">{detail.error.message}</MuiAlert>;
	}

	const incident = detail.data;
	const ongoing = incident.ended_at == null;

	const onDelete = async () => {
		try {
			await deleteAction.call({ id: incident.id });
			navigate("/incidents");
		} catch {
			/* surfaced via deleteAction.error */
		}
	};

	return (
		<Stack spacing={3}>
			<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
				<Typography variant="h4" component="h1" sx={{ flex: 1 }}>
					{incident.title}
				</Typography>
				<Chip size="small" variant="outlined" label="manual" />
				{ongoing ? (
					<Chip size="small" color="warning" label="ongoing" />
				) : (
					<Chip size="small" label="ended" />
				)}
				<Button size="small" onClick={() => setEditOpen(true)}>
					Edit
				</Button>
				<Button
					size="small"
					color="error"
					onClick={() => setConfirmDelete(true)}
				>
					Delete
				</Button>
			</Stack>

			<Typography variant="body2" color="text.secondary">
				<MuiLink
					component={RouterLink}
					to={`/groups/${incident.server_group_id}`}
					underline="hover"
				>
					{incident.server_group_name}
				</MuiLink>{" "}
				· started <TimeAgo timestamp={incident.started_at} />
				{incident.ended_at && (
					<>
						{" "}
						· ended <TimeAgo timestamp={incident.ended_at} />
					</>
				)}{" "}
				· recorded by {incident.created_by}
			</Typography>

			<Paper variant="outlined" sx={{ p: 2 }}>
				{incident.description ? (
					<Markdown>{incident.description}</Markdown>
				) : (
					<Typography variant="body2" color="text.secondary">
						No description recorded.
					</Typography>
				)}
			</Paper>

			<Typography variant="caption" color="text.secondary">
				Manual incidents are support-recorded history, written here or over
				the MCP interface. Last changed{" "}
				<TimeAgo timestamp={incident.updated_at} />.
			</Typography>

			<ManualIncidentFormDialog
				open={editOpen}
				onClose={() => setEditOpen(false)}
				onSaved={detail.reload}
				incident={incident}
			/>

			<Dialog open={confirmDelete} onClose={() => setConfirmDelete(false)}>
				<DialogTitle>Delete this manual incident?</DialogTitle>
				<DialogContent>
					<Stack spacing={2}>
						<Typography variant="body2">
							“{incident.title}” will be removed entirely; there is no
							undo.
						</Typography>
						{deleteAction.error && (
							<MuiAlert severity="error">
								{deleteAction.error.message}
							</MuiAlert>
						)}
					</Stack>
				</DialogContent>
				<DialogActions>
					<Button
						onClick={() => setConfirmDelete(false)}
						disabled={deleteAction.pending}
					>
						Cancel
					</Button>
					<Button
						color="error"
						variant="contained"
						onClick={onDelete}
						disabled={deleteAction.pending}
					>
						{deleteAction.pending ? "Deleting…" : "Delete"}
					</Button>
				</DialogActions>
			</Dialog>
		</Stack>
	);
}
