import {
	Alert as MuiAlert,
	Box,
	Button,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	IconButton,
	LinearProgress,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
import AddCommentIcon from "@mui/icons-material/AddComment";
import DeleteIcon from "@mui/icons-material/Delete";
import { useState } from "react";
import { useApi, useApiAction } from "../api";
import TimeAgo from "./TimeAgo";

/** Shared note shape; both IssueNoteData and IncidentNoteData fit this. */
interface NoteLike {
	id: string;
	author: string;
	body: string;
	created_at: string;
}

type ApiModule = "issues" | "incidents";
type ParentKey = "issue_id" | "incident_id";

/** Read-only list of notes scoped by `(apiModule, parentKey, parentId)`.
 * Pair with `AddNoteButton` for write access; bump `refreshKey` after a
 * successful add to refetch.
 */
export default function NotesList({
	apiModule,
	parentKey,
	parentId,
	refreshKey = 0,
}: {
	apiModule: ApiModule;
	parentKey: ParentKey;
	parentId: string;
	refreshKey?: number;
}) {
	const list = useApi(
		apiModule,
		"list_notes",
		{ [parentKey]: parentId },
		[apiModule, parentId, refreshKey],
	);

	if (list.status === "loading" || list.status === "idle") {
		return <LinearProgress />;
	}
	if (list.status === "error") {
		return <MuiAlert severity="error">{list.error.message}</MuiAlert>;
	}
	if (list.data.length === 0) {
		return (
			<Typography variant="caption" color="text.secondary">
				No notes yet.
			</Typography>
		);
	}
	return (
		<Stack spacing={1}>
			{list.data.map((n) => (
				<NoteRow
					key={n.id}
					note={n}
					apiModule={apiModule}
					onChanged={list.reload}
				/>
			))}
		</Stack>
	);
}

/** Button that opens a modal dialog containing the add-note form. */
export function AddNoteButton({
	apiModule,
	parentKey,
	parentId,
	onAdded,
	label = "Add note",
	variant = "text",
}: {
	apiModule: ApiModule;
	parentKey: ParentKey;
	parentId: string;
	onAdded?: () => void;
	label?: string;
	variant?: "text" | "outlined" | "contained";
}) {
	const [open, setOpen] = useState(false);
	const [draft, setDraft] = useState("");
	const add = useApiAction(apiModule, "add_note");

	const close = () => {
		setOpen(false);
		setDraft("");
	};
	const submit = async () => {
		const body = draft.trim();
		if (body === "") return;
		try {
			await add.call({ [parentKey]: parentId, body });
			setDraft("");
			setOpen(false);
			onAdded?.();
		} catch {
			/* surfaced via add.error */
		}
	};

	return (
		<>
			<Button
				size="small"
				variant={variant}
				startIcon={<AddCommentIcon />}
				onClick={() => setOpen(true)}
			>
				{label}
			</Button>
			<Dialog open={open} onClose={close} fullWidth maxWidth="sm">
				<DialogTitle>{label}</DialogTitle>
				<DialogContent>
					<TextField
						autoFocus
						fullWidth
						multiline
						minRows={3}
						value={draft}
						onChange={(e) => setDraft(e.target.value)}
						disabled={add.pending}
						sx={{ mt: 1 }}
					/>
					{add.error && (
						<MuiAlert severity="error" sx={{ mt: 1 }}>
							{add.error.message}
						</MuiAlert>
					)}
				</DialogContent>
				<DialogActions>
					<Button onClick={close} disabled={add.pending}>
						Cancel
					</Button>
					<Button
						variant="contained"
						onClick={submit}
						disabled={add.pending || draft.trim() === ""}
					>
						{add.pending ? "Adding…" : "Add"}
					</Button>
				</DialogActions>
			</Dialog>
		</>
	);
}

function NoteRow({
	note,
	apiModule,
	onChanged,
}: {
	note: NoteLike;
	apiModule: ApiModule;
	onChanged: () => void;
}) {
	const del = useApiAction(apiModule, "delete_note");

	const remove = async () => {
		try {
			await del.call({ note_id: note.id });
			onChanged();
		} catch {
			/* surfaced via del.error */
		}
	};

	return (
		<Box
			sx={{
				p: 1,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
				bgcolor: "background.paper",
			}}
		>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="caption" color="text.secondary">
					{note.author}
				</Typography>
				<Stack direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
					<Typography variant="caption" color="text.secondary">
						<TimeAgo timestamp={note.created_at} />
					</Typography>
					<IconButton
						size="small"
						color="error"
						aria-label="Delete"
						onClick={remove}
						disabled={del.pending}
					>
						<DeleteIcon fontSize="inherit" />
					</IconButton>
				</Stack>
			</Stack>
			<Typography
				variant="body2"
				component="pre"
				sx={{ mt: 0.5, mb: 0, whiteSpace: "pre-wrap", fontFamily: "inherit" }}
			>
				{note.body}
			</Typography>
			{del.error && (
				<MuiAlert severity="error" sx={{ mt: 1 }}>
					{del.error.message}
				</MuiAlert>
			)}
		</Box>
	);
}
