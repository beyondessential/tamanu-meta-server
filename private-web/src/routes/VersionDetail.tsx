import {
	Alert,
	Box,
	Button,
	IconButton,
	LinearProgress,
	MenuItem,
	Paper,
	Select,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	TextField,
	Typography,
} from "@mui/material";
import DeleteIcon from "@mui/icons-material/Delete";
import EditIcon from "@mui/icons-material/Edit";
import LockIcon from "@mui/icons-material/Lock";
import LockOpenIcon from "@mui/icons-material/LockOpen";
import { useState } from "react";
import { useParams } from "react-router-dom";
import Markdown from "../components/Markdown";
import VersionStatusChip from "../components/VersionStatusChip";
import { useApi, useApiAction } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { prettifyVersionRange } from "../lib/versionRange";
import type {
	ArtifactData,
	RelatedVersionData,
	VersionDetail as VersionDetailData,
	VersionStatus,
} from "../types";

export default function VersionDetail() {
	const { version = "" } = useParams<{ version: string }>();
	usePageTitle(version || "Version");
	const detail = useApi<VersionDetailData>(
		"versions",
		"get_version_detail",
		{ version },
		[version],
	);
	const isAdmin = useApi<boolean>("commons", "is_current_user_admin");

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <Alert severity="error">{detail.error.message}</Alert>;
	}

	const v = detail.data;
	const versionStr = `${v.major}.${v.minor}.${v.patch}`;
	const admin = isAdmin.status === "ok" && isAdmin.data;

	return (
		<Stack spacing={3}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h4" component="h1" sx={{ fontFamily: "monospace" }}>
					{versionStr}
				</Typography>
				<StatusControl
					detail={v}
					versionStr={versionStr}
					isAdmin={admin}
					onChanged={() => detail.reload()}
				/>
			</Stack>

			<VersionInfo detail={v} />

			<ArtifactsSection
				version={versionStr}
				versionId={v.id}
				isAdmin={admin}
			/>

			<ChangelogSection
				detail={v}
				versionStr={versionStr}
				isAdmin={admin}
				onChanged={() => detail.reload()}
			/>

			{v.related_versions.length > 0 && (
				<RelatedVersionsSection related={v.related_versions} />
			)}
		</Stack>
	);
}

function StatusControl({
	detail,
	versionStr,
	isAdmin,
	onChanged,
}: {
	detail: VersionDetailData;
	versionStr: string;
	isAdmin: boolean;
	onChanged: () => void;
}) {
	const [selected, setSelected] = useState<VersionStatus>(detail.status);
	const action = useApiAction("versions", "update_version_status");

	if (!isAdmin) {
		return <VersionStatusChip status={detail.status} />;
	}

	const canSwitchToDraft =
		detail.status !== "published" || detail.is_latest_in_minor;
	const dirty = selected !== detail.status;

	return (
		<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
			<Select
				size="small"
				value={selected}
				onChange={(e) => setSelected(e.target.value as VersionStatus)}
				disabled={action.pending}
			>
				<MenuItem value="draft" disabled={!canSwitchToDraft}>
					Draft
				</MenuItem>
				<MenuItem value="published">Published</MenuItem>
				<MenuItem value="yanked">Yanked</MenuItem>
			</Select>
			<Button
				variant="contained"
				disabled={!dirty || action.pending}
				onClick={async () => {
					try {
						await action.call({ version: versionStr, status: selected });
						onChanged();
					} catch {
						/* surfaced via action.error */
					}
				}}
			>
				{action.pending ? "Changing…" : "Change"}
			</Button>
			{action.error && (
				<Typography variant="caption" color="error">
					{action.error.message}
				</Typography>
			)}
		</Stack>
	);
}

function ChangelogSection({
	detail,
	versionStr,
	isAdmin,
	onChanged,
}: {
	detail: VersionDetailData;
	versionStr: string;
	isAdmin: boolean;
	onChanged: () => void;
}) {
	const [editing, setEditing] = useState(false);
	const [draft, setDraft] = useState(detail.changelog);
	const action = useApiAction("versions", "update_version_changelog");

	const start = () => {
		setDraft(detail.changelog);
		setEditing(true);
	};
	const cancel = () => {
		setDraft(detail.changelog);
		setEditing(false);
	};
	const save = async () => {
		try {
			await action.call({ version: versionStr, changelog: draft });
			setEditing(false);
			onChanged();
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<Box>
			<Stack
				direction="row"
				spacing={1}
				sx={{ mb: 1, alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h5" component="h2">
					Changelog
				</Typography>
				{isAdmin &&
					(editing ? (
						<Stack direction="row" spacing={1}>
							<Button
								variant="contained"
								color="success"
								onClick={save}
								disabled={action.pending}
							>
								{action.pending ? "Saving…" : "Save"}
							</Button>
							<Button
								variant="outlined"
								color="error"
								onClick={cancel}
								disabled={action.pending}
							>
								Cancel
							</Button>
						</Stack>
					) : (
						<Button
							variant="outlined"
							startIcon={<EditIcon />}
							onClick={start}
						>
							Edit
						</Button>
					))}
			</Stack>
			<Paper variant="outlined" sx={{ p: 2 }}>
				{editing ? (
					<TextField
						multiline
						fullWidth
						minRows={20}
						value={draft}
						onChange={(e) => setDraft(e.target.value)}
						slotProps={{
							input: { sx: { fontFamily: "monospace" } },
						}}
					/>
				) : detail.changelog ? (
					<Markdown>{detail.changelog}</Markdown>
				) : (
					<Typography variant="body2" color="text.secondary">
						No changelog
					</Typography>
				)}
			</Paper>
			{action.error && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{action.error.message}
				</Alert>
			)}
		</Box>
	);
}

function VersionInfo({ detail }: { detail: VersionDetailData }) {
	const items = [
		{ label: "Created", value: formatDate(detail.created_at) },
		{ label: "Last updated", value: formatDate(detail.updated_at) },
		detail.min_chrome_version != null && {
			label: "Chrome support",
			value: `${detail.min_chrome_version} or later`,
		},
	].filter(Boolean) as Array<{ label: string; value: string }>;

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={4}
				useFlexGap
				sx={{ flexWrap: "wrap" }}
			>
				{items.map(({ label, value }) => (
					<Stack key={label} spacing={0.25}>
						<Typography variant="caption" color="text.secondary">
							{label}
						</Typography>
						<Typography variant="body2">{value}</Typography>
					</Stack>
				))}
			</Stack>
		</Paper>
	);
}

function ArtifactsSection({
	version,
	versionId,
	isAdmin,
}: {
	version: string;
	versionId: string;
	isAdmin: boolean;
}) {
	const result = useApi<ArtifactData[]>(
		"versions",
		"get_version_artifacts",
		{ version },
		[version],
	);
	const [unlocked, setUnlocked] = useState(false);
	const [showCreate, setShowCreate] = useState(false);

	const reload = () => result.reload();

	return (
		<Box>
			<Stack
				direction="row"
				spacing={1}
				sx={{ mb: 1, alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h5" component="h2">
					Artifacts
				</Typography>
				{isAdmin && (
					<Stack direction="row" spacing={1}>
						{unlocked && (
							<Button
								variant={showCreate ? "outlined" : "contained"}
								color={showCreate ? "warning" : "primary"}
								onClick={() => setShowCreate((s) => !s)}
							>
								{showCreate ? "Cancel create" : "Create"}
							</Button>
						)}
						<Button
							variant="outlined"
							startIcon={unlocked ? <LockOpenIcon /> : <LockIcon />}
							onClick={() => setUnlocked((u) => !u)}
						>
							{unlocked ? "Lock" : "Unlock"}
						</Button>
					</Stack>
				)}
			</Stack>
			{showCreate && (
				<Box sx={{ mb: 2 }}>
					<CreateArtifactForm
						versionId={versionId}
						onCreated={() => {
							setShowCreate(false);
							reload();
						}}
					/>
				</Box>
			)}
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : result.data.length === 0 ? (
				<Alert severity="info">No artifacts for this version.</Alert>
			) : (
				<TableContainer component={Paper} variant="outlined">
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell>Type</TableCell>
								<TableCell>Platform</TableCell>
								<TableCell>Download URL</TableCell>
								{unlocked && <TableCell />}
							</TableRow>
						</TableHead>
						<TableBody>
							{result.data.map((a) => (
								<ArtifactRow
									key={a.id}
									artifact={a}
									unlocked={unlocked}
									onChanged={reload}
								/>
							))}
						</TableBody>
					</Table>
				</TableContainer>
			)}
		</Box>
	);
}

function ArtifactRow({
	artifact,
	unlocked,
	onChanged,
}: {
	artifact: ArtifactData;
	unlocked: boolean;
	onChanged: () => void;
}) {
	const [editing, setEditing] = useState(false);
	const [confirmDelete, setConfirmDelete] = useState(false);
	const deleteAction = useApiAction("versions", "delete_artifact");

	if (editing) {
		return (
			<EditArtifactRow
				artifact={artifact}
				onClose={(changed) => {
					setEditing(false);
					if (changed) onChanged();
				}}
			/>
		);
	}

	const onDelete = async () => {
		try {
			await deleteAction.call({ artifact_id: artifact.id });
			onChanged();
		} catch {
			/* surfaced via deleteAction.error */
		}
	};

	return (
		<TableRow>
			<TableCell sx={{ fontFamily: "monospace" }}>
				{artifact.artifact_type}
				{artifact.has_range_override && (
					<Typography
						variant="caption"
						color="warning.main"
						sx={{ display: "block" }}
					>
						Overrides other artifact
					</Typography>
				)}
				{artifact.version_range_pattern && (
					<Typography
						variant="caption"
						color="text.secondary"
						sx={{ display: "block" }}
					>
						{!artifact.is_used_in_public_api && (
							<Box component="span" sx={{ color: "error.main", mr: 0.5 }}>
								[Hidden]
							</Box>
						)}
						Applies to: {prettifyVersionRange(artifact.version_range_pattern)}
					</Typography>
				)}
			</TableCell>
			<TableCell sx={{ fontFamily: "monospace" }}>
				{artifact.platform}
			</TableCell>
			<TableCell sx={{ wordBreak: "break-all" }}>
				{artifact.download_url.startsWith("https://") ? (
					<a
						href={artifact.download_url}
						target="_blank"
						rel="noopener noreferrer"
					>
						{artifact.download_url}
					</a>
				) : (
					artifact.download_url
				)}
			</TableCell>
			{unlocked && (
				<TableCell align="right">
					{confirmDelete ? (
						<Stack direction="row" spacing={0.5} sx={{ justifyContent: "flex-end" }}>
							<Button
								size="small"
								variant="contained"
								color="error"
								onClick={onDelete}
								disabled={deleteAction.pending}
							>
								Really delete
							</Button>
							<Button
								size="small"
								variant="outlined"
								onClick={() => setConfirmDelete(false)}
								disabled={deleteAction.pending}
							>
								Cancel
							</Button>
						</Stack>
					) : (
						<Stack direction="row" spacing={0.5} sx={{ justifyContent: "flex-end" }}>
							<IconButton
								aria-label={`edit ${artifact.artifact_type}`}
								size="small"
								onClick={() => setEditing(true)}
							>
								<EditIcon fontSize="small" />
							</IconButton>
							<IconButton
								aria-label={`delete ${artifact.artifact_type}`}
								size="small"
								color="error"
								onClick={() => setConfirmDelete(true)}
							>
								<DeleteIcon fontSize="small" />
							</IconButton>
						</Stack>
					)}
				</TableCell>
			)}
		</TableRow>
	);
}

function EditArtifactRow({
	artifact,
	onClose,
}: {
	artifact: ArtifactData;
	onClose: (changed: boolean) => void;
}) {
	const [type, setType] = useState(artifact.artifact_type);
	const [platform, setPlatform] = useState(artifact.platform);
	const [url, setUrl] = useState(artifact.download_url);
	const action = useApiAction("versions", "update_artifact");

	const save = async () => {
		try {
			await action.call({
				artifact_id: artifact.id,
				artifact_type: type,
				platform,
				download_url: url,
			});
			onClose(true);
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<TableRow>
			<TableCell>
				<TextField
					size="small"
					value={type}
					onChange={(e) => setType(e.target.value)}
					disabled={action.pending}
					required
				/>
			</TableCell>
			<TableCell>
				<TextField
					size="small"
					value={platform}
					onChange={(e) => setPlatform(e.target.value)}
					disabled={action.pending}
					required
				/>
			</TableCell>
			<TableCell>
				<TextField
					size="small"
					fullWidth
					value={url}
					onChange={(e) => setUrl(e.target.value)}
					disabled={action.pending}
					required
				/>
			</TableCell>
			<TableCell align="right">
				<Stack direction="row" spacing={0.5} sx={{ justifyContent: "flex-end" }}>
					<Button
						size="small"
						variant="contained"
						onClick={save}
						disabled={action.pending}
					>
						{action.pending ? "Saving…" : "Save"}
					</Button>
					<Button
						size="small"
						variant="outlined"
						onClick={() => onClose(false)}
						disabled={action.pending}
					>
						Cancel
					</Button>
				</Stack>
			</TableCell>
		</TableRow>
	);
}

function CreateArtifactForm({
	versionId,
	onCreated,
}: {
	versionId: string;
	onCreated: () => void;
}) {
	const [type, setType] = useState("");
	const [platform, setPlatform] = useState("");
	const [url, setUrl] = useState("");
	const action = useApiAction("versions", "create_artifact");

	const submit = async (e: React.FormEvent) => {
		e.preventDefault();
		try {
			await action.call({
				version_id: versionId,
				artifact_type: type,
				platform,
				download_url: url,
			});
			setType("");
			setPlatform("");
			setUrl("");
			onCreated();
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Box component="form" onSubmit={submit}>
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "flex-start" }}
				>
					<TextField
						size="small"
						label="Type"
						value={type}
						onChange={(e) => setType(e.target.value)}
						disabled={action.pending}
						required
					/>
					<TextField
						size="small"
						label="Platform"
						value={platform}
						onChange={(e) => setPlatform(e.target.value)}
						disabled={action.pending}
						required
					/>
					<TextField
						size="small"
						label="Download URL"
						value={url}
						onChange={(e) => setUrl(e.target.value)}
						disabled={action.pending}
						fullWidth
						required
					/>
					<Button
						type="submit"
						variant="contained"
						disabled={action.pending}
					>
						{action.pending ? "Creating…" : "Create"}
					</Button>
				</Stack>
				{action.error && (
					<Alert severity="error" sx={{ mt: 1 }}>
						{action.error.message}
					</Alert>
				)}
			</Box>
		</Paper>
	);
}

function RelatedVersionsSection({
	related,
}: {
	related: RelatedVersionData[];
}) {
	return (
		<Box>
			<Typography variant="h5" component="h2" gutterBottom>
				Related versions
			</Typography>
			<Stack spacing={2}>
				{related.map((r) => (
					<Box key={`${r.major}.${r.minor}.${r.patch}`}>
						<Typography
							variant="h6"
							component="h3"
							sx={{ fontFamily: "monospace" }}
						>
							{r.major}.{r.minor}.{r.patch}
						</Typography>
						<Paper variant="outlined" sx={{ p: 2 }}>
							{r.changelog ? (
								<Markdown>{r.changelog}</Markdown>
							) : (
								<Typography variant="body2" color="text.secondary">
									No changelog
								</Typography>
							)}
						</Paper>
					</Box>
				))}
			</Stack>
		</Box>
	);
}

function formatDate(iso: string): string {
	return iso.slice(0, 10);
}
