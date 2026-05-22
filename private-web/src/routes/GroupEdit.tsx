import {
	Alert,
	Button,
	LinearProgress,
	Paper,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
import DeleteIcon from "@mui/icons-material/DeleteOutlined";
import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import TagsEditor from "../components/TagsEditor";
import { useApi, useApiAction } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import type { ServerGroup, TagMap } from "../types";

export default function GroupEdit() {
	const { id = "" } = useParams<{ id: string }>();
	usePageTitle("Edit group");
	const result = useApi("server_groups", "get", { server_group_id: id }, [id]);

	if (result.status === "loading" || result.status === "idle") {
		return <LinearProgress />;
	}
	if (result.status === "error") {
		return <Alert severity="error">{result.error.message}</Alert>;
	}
	return (
		<EditForm
			group={result.data.group}
			memberCount={result.data.servers.length}
		/>
	);
}

function EditForm({
	group,
	memberCount,
}: {
	group: ServerGroup;
	memberCount: number;
}) {
	const navigate = useNavigate();
	const update = useApiAction("server_groups", "update");
	const remove = useApiAction("server_groups", "delete");

	const [name, setName] = useState(group.name);
	const [notes, setNotes] = useState(group.notes ?? "");
	const [tags, setTags] = useState<TagMap>(group.tags ?? {});

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		try {
			await update.call({
				server_group_id: group.id,
				data: { name, notes, tags },
			});
			navigate(`/groups/${group.id}`);
		} catch {
			/* surfaced via update.error */
		}
	};

	const onDelete = async () => {
		if (memberCount > 0) {
			alert(
				`This group still has ${memberCount} server(s). Move them out before deleting.`,
			);
			return;
		}
		if (!confirm(`Delete group "${group.name}"? This cannot be undone.`)) return;
		try {
			await remove.call({ server_group_id: group.id });
			navigate("/servers");
		} catch {
			/* surfaced via remove.error */
		}
	};

	const pending = update.pending || remove.pending;

	return (
		<Paper variant="outlined" sx={{ p: 3 }} component="form" onSubmit={onSubmit}>
			<Stack spacing={2}>
				<Typography variant="h5" component="h1">
					Edit group
				</Typography>

				<TextField
					label="Name"
					value={name}
					onChange={(e) => setName(e.target.value)}
					disabled={pending}
					required
				/>

				<TextField
					label="Notes"
					multiline
					minRows={3}
					value={notes}
					onChange={(e) => setNotes(e.target.value)}
					disabled={pending}
					helperText="Plain text shown on the group's detail page."
				/>

				<Stack spacing={1}>
					<Typography variant="subtitle1">Tags</Typography>
					<TagsEditor value={tags} onChange={setTags} disabled={pending} />
					<Typography variant="caption" color="text.secondary">
						These tags are inherited by every server in the group. A server's
						own tags override the group's on the public tags endpoint.
					</Typography>
				</Stack>

				{(update.error || remove.error) && (
					<Alert severity="error">
						{(update.error || remove.error)!.message}
					</Alert>
				)}

				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center", justifyContent: "space-between" }}
				>
					<Stack direction="row" spacing={1}>
						<Button
							type="submit"
							variant="contained"
							disabled={pending}
						>
							{update.pending ? "Saving…" : "Save"}
						</Button>
						<Button
							type="button"
							variant="outlined"
							color="error"
							onClick={() => navigate(`/groups/${group.id}`)}
							disabled={pending}
						>
							Cancel
						</Button>
					</Stack>
					<Button
						type="button"
						variant="outlined"
						color="error"
						startIcon={<DeleteIcon />}
						onClick={onDelete}
						disabled={pending || memberCount > 0}
					>
						Delete group
					</Button>
				</Stack>
			</Stack>
		</Paper>
	);
}
