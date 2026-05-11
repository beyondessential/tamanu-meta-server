import {
	Alert as MuiAlert,
	Box,
	Button,
	IconButton,
	LinearProgress,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
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

/** Generic notes panel — works for both issues and incidents.
 *
 * The caller picks the API module (`"issues"` or `"incidents"`) and the
 * key/id used to scope notes. Notes are immutable once written; "edit"
 * = delete + add a new one.
 */
export default function NotesPanel({
	apiModule,
	parentKey,
	parentId,
}: {
	apiModule: "issues" | "incidents";
	parentKey: "issue_id" | "incident_id";
	parentId: string;
}) {
	const list = useApi<NoteLike[]>(
		apiModule,
		"list_notes",
		{ [parentKey]: parentId },
		[apiModule, parentId],
	);
	const add = useApiAction(apiModule, "add_note");
	const [draft, setDraft] = useState("");

	const submit = async () => {
		const body = draft.trim();
		if (body === "") return;
		try {
			await add.call({ [parentKey]: parentId, body });
			setDraft("");
			list.reload();
		} catch {
			/* surfaced via add.error */
		}
	};

	return (
		<Box>
			<Stack spacing={1} sx={{ mb: 1 }}>
				<TextField
					label="Add a note"
					size="small"
					multiline
					minRows={2}
					value={draft}
					onChange={(e) => setDraft(e.target.value)}
					disabled={add.pending}
				/>
				<Box>
					<Button
						variant="contained"
						size="small"
						onClick={submit}
						disabled={add.pending || draft.trim() === ""}
					>
						{add.pending ? "Adding…" : "Add note"}
					</Button>
				</Box>
				{add.error && <MuiAlert severity="error">{add.error.message}</MuiAlert>}
			</Stack>
			{list.status === "loading" || list.status === "idle" ? (
				<LinearProgress />
			) : list.status === "error" ? (
				<MuiAlert severity="error">{list.error.message}</MuiAlert>
			) : list.data.length === 0 ? (
				<Typography variant="caption" color="text.secondary">
					No notes yet.
				</Typography>
			) : (
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
			)}
		</Box>
	);
}

function NoteRow({
	note,
	apiModule,
	onChanged,
}: {
	note: NoteLike;
	apiModule: "issues" | "incidents";
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
					{note.author} · <TimeAgo timestamp={note.created_at} />
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
