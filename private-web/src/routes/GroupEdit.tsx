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
	const { id } = useParams<{ id?: string }>();
	const isCreate = id == null;
	usePageTitle(isCreate ? "New group" : "Edit group");
	const result = useApi(
		"server_groups",
		"get",
		{ server_group_id: id ?? "" },
		[id ?? ""],
	);

	if (isCreate) return <CreateForm />;

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

/// Create mode. Collects just a name (notes/tags/cooldown are editable
/// afterwards) and, on success, drops the operator straight into the
/// add-server flow for the freshly-created group.
function CreateForm() {
	const navigate = useNavigate();
	const create = useApiAction("server_groups", "create");
	const [name, setName] = useState("");
	const [notes, setNotes] = useState("");
	const [tags, setTags] = useState<TagMap>({});

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		try {
			const group = await create.call({ name, notes, tags });
			navigate(`/groups/${group.id}/servers/new`);
		} catch {
			/* surfaced via create.error */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 3 }} component="form" onSubmit={onSubmit}>
			<Stack spacing={2}>
				<Typography variant="h5" component="h1">
					New group
				</Typography>

				<TextField
					label="Name"
					value={name}
					onChange={(e) => setName(e.target.value)}
					disabled={create.pending}
					required
				/>

				<TextField
					label="Notes"
					multiline
					minRows={3}
					value={notes}
					onChange={(e) => setNotes(e.target.value)}
					disabled={create.pending}
					helperText="Plain text shown on the group's detail page."
				/>

				<Stack spacing={1}>
					<Typography variant="subtitle1">Tags</Typography>
					<TagsEditor value={tags} onChange={setTags} disabled={create.pending} />
					<Typography variant="caption" color="text.secondary">
						These tags are inherited by every server in the group.
					</Typography>
				</Stack>

				{create.error && (
					<Alert severity="error">{create.error.message}</Alert>
				)}

				<Stack direction="row" spacing={1}>
					<Button type="submit" variant="contained" disabled={create.pending}>
						{create.pending ? "Creating…" : "Create group"}
					</Button>
					<Button
						type="button"
						variant="outlined"
						color="error"
						onClick={() => navigate("/servers")}
						disabled={create.pending}
					>
						Cancel
					</Button>
				</Stack>
			</Stack>
		</Paper>
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
	// Slack open cooldown: how long a newly-opened incident's Slack notice
	// sits in the outbox before the drainer is allowed to ship it. UI works
	// in minutes; 0 = ship immediately.
	const [slackOpenDelayMinutes, setSlackOpenDelayMinutes] = useState<string>(
		Math.max(0, Math.round(group.slack_open_delay / 60)).toString(),
	);

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		try {
			await update.call({
				server_group_id: group.id,
				data: {
					name,
					notes,
					tags,
					slack_open_delay: Math.max(
						0,
						Math.round(Number(slackOpenDelayMinutes) * 60),
					),
				},
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
					<Typography variant="subtitle1">Slack cooldown</Typography>
					<Stack
						direction={{ xs: "column", md: "row" }}
						spacing={2}
						sx={{ alignItems: { md: "center" } }}
					>
						<Typography variant="body2">
							Hold incident-open Slack notices for
						</Typography>
						<TextField
							label="minutes"
							type="number"
							value={slackOpenDelayMinutes}
							onChange={(e) => setSlackOpenDelayMinutes(e.target.value)}
							disabled={pending}
							slotProps={{ htmlInput: { min: 0, step: 1 } }}
							sx={{ width: 140 }}
						/>
					</Stack>
					<Typography variant="caption" color="text.secondary">
						If the incident resolves inside the window, no Slack notice is
						sent for either edge — useful for flappy probes. Set to 0 to ship
						opens immediately.
					</Typography>
				</Stack>

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
