import {
	Alert,
	Box,
	Button,
	LinearProgress,
	Link as MuiLink,
	Pagination,
	Paper,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import type { BestoolSnippetInfo } from "../types";

const PAGE_SIZE = 10;

export default function BestoolSnippets() {
	const [page, setPage] = useState(0);
	const [showCreate, setShowCreate] = useState(false);

	const count = useApi<number>("bestool", "count_snippets");
	const list = useApi<BestoolSnippetInfo[]>(
		"bestool",
		"list_snippets",
		{ offset: page * PAGE_SIZE, limit: PAGE_SIZE },
		[page],
	);

	const refresh = () => {
		count.reload();
		list.reload();
	};

	const total = count.status === "ok" ? count.data : 0;
	const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

	return (
		<Stack spacing={3}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h4" component="h1">
					PSQL Snippets
				</Typography>
				<Button
					variant={showCreate ? "outlined" : "contained"}
					color={showCreate ? "error" : "primary"}
					onClick={() => setShowCreate((s) => !s)}
				>
					{showCreate ? "Cancel" : "Add"}
				</Button>
			</Stack>

			{showCreate && (
				<CreateSnippetForm
					onCreated={() => {
						setShowCreate(false);
						refresh();
					}}
				/>
			)}

			{list.status === "loading" || list.status === "idle" ? (
				<LinearProgress />
			) : list.status === "error" ? (
				<Alert severity="error">{list.error.message}</Alert>
			) : list.data.length === 0 ? (
				<Alert severity="info">No snippets found.</Alert>
			) : (
				<Stack spacing={1}>
					{list.data.map((s) => (
						<MuiLink
							key={s.id}
							component={RouterLink}
							to={`/bestool/snippets/${s.id}`}
							underline="none"
							color="inherit"
						>
							<Paper
								variant="outlined"
								sx={{ p: 1.5, "&:hover": { boxShadow: 1 } }}
							>
								<Typography variant="body1" sx={{ fontFamily: "monospace" }}>
									{s.name}
								</Typography>
								{s.description && (
									<Typography variant="body2" color="text.secondary">
										{s.description}
									</Typography>
								)}
							</Paper>
						</MuiLink>
					))}
				</Stack>
			)}
			{pageCount > 1 && (
				<Box sx={{ display: "flex", justifyContent: "center" }}>
					<Pagination
						count={pageCount}
						page={page + 1}
						onChange={(_, p) => setPage(p - 1)}
					/>
				</Box>
			)}
		</Stack>
	);
}

function CreateSnippetForm({ onCreated }: { onCreated: () => void }) {
	const [name, setName] = useState("");
	const [description, setDescription] = useState("");
	const [sql, setSql] = useState("");
	const action = useApiAction("bestool", "create_snippet");

	const submit = async (e: React.FormEvent) => {
		e.preventDefault();
		try {
			await action.call({
				name,
				description: description.trim() === "" ? null : description,
				sql,
			});
			setName("");
			setDescription("");
			setSql("");
			onCreated();
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }} component="form" onSubmit={submit}>
			<Stack spacing={2}>
				<TextField
					label="Name (will become the filename/snipname)"
					placeholder="example_snippet_name"
					value={name}
					onChange={(e) => setName(e.target.value)}
					disabled={action.pending}
					required
				/>
				<TextField
					label="Description (optional)"
					placeholder="Sentence about what it does and ${variables} required"
					value={description}
					onChange={(e) => setDescription(e.target.value)}
					disabled={action.pending}
					multiline
					minRows={2}
				/>
				<TextField
					label="SQL (no sensitive info! everything here may be read by anyone with bestool)"
					placeholder="SELECT ..."
					value={sql}
					onChange={(e) => setSql(e.target.value)}
					disabled={action.pending}
					required
					multiline
					minRows={6}
					slotProps={{
						input: { sx: { fontFamily: "monospace" } },
					}}
				/>
				{action.error && (
					<Alert severity="error">{action.error.message}</Alert>
				)}
				<Box>
					<Button
						type="submit"
						variant="contained"
						disabled={action.pending}
					>
						{action.pending ? "Saving…" : "Save"}
					</Button>
				</Box>
			</Stack>
		</Paper>
	);
}
