import {
	Alert,
	Box,
	Button,
	Chip,
	CircularProgress,
	Dialog,
	DialogActions,
	DialogContent,
	DialogContentText,
	DialogTitle,
	Divider,
	FormControlLabel,
	LinearProgress,
	Paper,
	Stack,
	Switch,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableRow,
	TextField,
	Typography,
} from "@mui/material";
import { useState } from "react";
import BackupIcon from "@mui/icons-material/Backup";
import DeleteIcon from "@mui/icons-material/Delete";
import EditIcon from "@mui/icons-material/Edit";
import RefreshIcon from "@mui/icons-material/Refresh";
import { Link as RouterLink, useParams } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { humanSeconds } from "../lib/humanDuration";
import { usePageTitle } from "../hooks/usePageTitle";
import TimeAgo from "../components/TimeAgo";
import {
	BACKUP_STATUS_HELP,
	BACKUP_STATUS_INTENT,
	BACKUP_STATUS_LABEL,
	type BackupConfigStatus,
	type BackupConfigView,
	type ServerInfo,
} from "../types";

const WELL_KNOWN_TYPE = "tamanu-postgres";

export default function BackupPanel() {
	const { id = "" } = useParams<{ id: string }>();
	usePageTitle("Backups");
	const isAdmin = useIsAdmin() === true;
	const config = useApi(
		"backups",
		"get",
		{ server_group_id: id },
		[id],
	);
	// Poll while provisioning so the operator sees the init Job land.
	const provisioning =
		config.status === "ok" && config.data?.status === "provisioning";
	const tick = useReloadInterval(provisioning ? 5_000 : 30_000, "canopy-data-changed");
	const configForTick = useApi(
		"backups",
		"get",
		{ server_group_id: id },
		[id, tick],
	);
	const group = useApi(
		"server_groups",
		"get",
		{ server_group_id: id },
		[id],
	);

	const data =
		configForTick.status === "ok"
			? configForTick.data
			: config.status === "ok"
				? config.data
				: undefined;

	if (config.status === "loading" || config.status === "idle") {
		return <LinearProgress />;
	}
	if (config.status === "error") {
		return <Alert severity="error">{config.error.message}</Alert>;
	}

	if (data == null) {
		return (
			<Stack spacing={2}>
				<Typography variant="h4" component="h1">
					Backups
				</Typography>
				<Alert severity="info">Backups not set up for this group.</Alert>
				{isAdmin && (
					<Box>
						<Button
							component={RouterLink}
							to={`/groups/${id}/backups/config`}
							variant="contained"
							startIcon={<BackupIcon />}
						>
							Set up backups
						</Button>
					</Box>
				)}
			</Stack>
		);
	}

	const members =
		group.status === "ok" ? group.data.servers : [];
	const status = data.status as BackupConfigStatus;

	return (
		<Stack spacing={3}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Stack direction="row" spacing={1.5} sx={{ alignItems: "center" }}>
					<Typography variant="h4" component="h1">
						Backups
					</Typography>
					<Chip
						label={BACKUP_STATUS_LABEL[status]}
						color={BACKUP_STATUS_INTENT[status]}
						size="small"
					/>
				</Stack>
				{isAdmin && (
					<Stack direction="row" spacing={1}>
						<Button
							component={RouterLink}
							to={`/groups/${id}/backups/config`}
							variant="outlined"
							startIcon={<EditIcon />}
						>
							Edit config
						</Button>
						<DeleteConfigButton groupId={id} onDeleted={configForTick.reload} />
					</Stack>
				)}
			</Stack>

			<Alert severity={BACKUP_STATUS_INTENT[status]}>
				{BACKUP_STATUS_HELP[status]}
			</Alert>

			<ConfigSummary config={data} />

			{status === "provisioning" && (
				<ProvisioningCard
					config={data}
					isAdmin={isAdmin}
					onRetried={configForTick.reload}
				/>
			)}

			{status === "ready" && (
				<>
					<SchedulesPanel groupId={id} isAdmin={isAdmin} />
					<StatsPanel groupId={id} />
					<RunsAndRequests
						groupId={id}
						members={members}
						isAdmin={isAdmin}
					/>
				</>
			)}
		</Stack>
	);
}

/// Decommission a group's backup config (admin). Deletes the config row + the
/// Canopy-owned passphrase Secret; the bucket and its object-locked backups are
/// untouched. Confirms first since it stops credential issuance for the group.
function DeleteConfigButton({
	groupId,
	onDeleted,
}: {
	groupId: string;
	onDeleted: () => void;
}) {
	const [open, setOpen] = useState(false);
	const del = useApiAction("backups", "delete");

	const onConfirm = async () => {
		try {
			await del.call({ server_group_id: groupId });
			setOpen(false);
			onDeleted();
		} catch {
			/* surfaced via del.error */
		}
	};

	return (
		<>
			<Button
				variant="outlined"
				color="error"
				startIcon={<DeleteIcon />}
				onClick={() => setOpen(true)}
			>
				Delete
			</Button>
			<Dialog open={open} onClose={() => !del.pending && setOpen(false)}>
				<DialogTitle>Delete backup config?</DialogTitle>
				<DialogContent>
					<DialogContentText>
						This stops credential issuance for the group and deletes the
						Canopy-owned repository passphrase. The bucket and its (object-locked)
						backups are left untouched — you can set backups up again afterwards.
					</DialogContentText>
					{del.error && (
						<Alert severity="error" sx={{ mt: 2 }}>
							{del.error.message}
						</Alert>
					)}
				</DialogContent>
				<DialogActions>
					<Button onClick={() => setOpen(false)} disabled={del.pending}>
						Cancel
					</Button>
					<Button
						color="error"
						variant="contained"
						onClick={onConfirm}
						disabled={del.pending}
					>
						{del.pending ? "Deleting…" : "Delete"}
					</Button>
				</DialogActions>
			</Dialog>
		</>
	);
}

function ConfigSummary({ config }: { config: BackupConfigView }) {
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack spacing={0.5}>
				<Typography variant="body2">
					<strong>Bucket:</strong> {config.bucket}
					{config.prefix ? `/${config.prefix}` : ""}
				</Typography>
				<Typography variant="body2">
					<strong>Region:</strong> {config.region ?? "default"}
				</Typography>
				<Typography variant="body2">
					<strong>Placement:</strong>{" "}
					{config.placement === "shared"
						? "canopy-created bucket in the shared account"
						: "existing bucket in a dedicated account"}
				</Typography>
			</Stack>
		</Paper>
	);
}

/// Per backup type, the group's effective schedule + retention (inherited from
/// the canopy-wide default, or a per-group override). Admins can override a type
/// or reset it back to the default.
function SchedulesPanel({
	groupId,
	isAdmin,
}: {
	groupId: string;
	isAdmin: boolean;
}) {
	const schedules = useApi(
		"backups",
		"group_schedules",
		{ server_group_id: groupId },
		[groupId],
	);

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="h6" component="h2" gutterBottom>
				Schedule &amp; retention
			</Typography>
			{schedules.status === "loading" || schedules.status === "idle" ? (
				<LinearProgress />
			) : schedules.status === "error" ? (
				<Alert severity="error">{schedules.error.message}</Alert>
			) : schedules.data.length === 0 ? (
				<Alert severity="info">
					No backup types are enabled for this group's servers yet. Each type
					inherits the canopy-wide default until a server advertises it.
				</Alert>
			) : (
				<Stack spacing={2}>
					{schedules.data.map((s) => (
						<TypeSchedule
							key={s.type}
							groupId={groupId}
							schedule={s}
							isAdmin={isAdmin}
							onChanged={schedules.reload}
						/>
					))}
				</Stack>
			)}
		</Paper>
	);
}

type GroupTypeSchedule = {
	type: string;
	effective_interval: number | null;
	effective_retention: {
		keep_latest: number;
		keep_daily: number;
		keep_weekly: number;
		keep_monthly: number;
		keep_annual: number;
	};
	has_override: boolean;
};

function TypeSchedule({
	groupId,
	schedule,
	isAdmin,
	onChanged,
}: {
	groupId: string;
	schedule: GroupTypeSchedule;
	isAdmin: boolean;
	onChanged: () => void;
}) {
	const [editing, setEditing] = useState(false);
	const r = schedule.effective_retention;

	return (
		<Box sx={{ borderTop: 1, borderColor: "divider", pt: 1.5 }}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
			>
				<Typography sx={{ fontFamily: "monospace" }}>{schedule.type}</Typography>
				<Chip
					size="small"
					label={schedule.has_override ? "Override" : "Inherited default"}
					color={schedule.has_override ? "secondary" : "default"}
					variant={schedule.has_override ? "filled" : "outlined"}
				/>
				<Box sx={{ flex: 1 }} />
				{isAdmin && !editing && (
					<Button size="small" onClick={() => setEditing(true)}>
						{schedule.has_override ? "Edit override" : "Override"}
					</Button>
				)}
			</Stack>
			<Typography variant="body2" color="text.secondary">
				{schedule.effective_interval != null
					? `Every ${humanSeconds(schedule.effective_interval)}`
					: "Manual only (no scheduled interval)"}{" "}
				· retention latest {r.keep_latest}, daily {r.keep_daily}, weekly{" "}
				{r.keep_weekly}, monthly {r.keep_monthly}, annual {r.keep_annual}
			</Typography>
			{editing && (
				<OverrideEditor
					groupId={groupId}
					schedule={schedule}
					onDone={() => {
						setEditing(false);
						onChanged();
					}}
					onCancel={() => setEditing(false)}
				/>
			)}
		</Box>
	);
}

const RETENTION_FIELDS: Array<{
	key: keyof GroupTypeSchedule["effective_retention"];
	label: string;
	floor?: number;
}> = [
	{ key: "keep_latest", label: "Latest" },
	{ key: "keep_daily", label: "Daily", floor: 7 },
	{ key: "keep_weekly", label: "Weekly", floor: 4 },
	{ key: "keep_monthly", label: "Monthly", floor: 6 },
	{ key: "keep_annual", label: "Annual" },
];

function OverrideEditor({
	groupId,
	schedule,
	onDone,
	onCancel,
}: {
	groupId: string;
	schedule: GroupTypeSchedule;
	onDone: () => void;
	onCancel: () => void;
}) {
	const setSchedule = useApiAction("backups", "set_schedule");
	const clearSchedule = useApiAction("backups", "clear_schedule");
	const [scheduled, setScheduled] = useState(
		schedule.effective_interval != null,
	);
	const [hours, setHours] = useState(
		schedule.effective_interval != null
			? String(Math.max(1, Math.round(schedule.effective_interval / 3600)))
			: "6",
	);
	const [retention, setRetention] = useState(schedule.effective_retention);

	const floorError = RETENTION_FIELDS.filter(
		(f) => f.floor != null && retention[f.key] < f.floor,
	).map((f) => `${f.label} must be ≥ ${f.floor}`);

	const save = async () => {
		await setSchedule.call({
			server_group_id: groupId,
			type: schedule.type,
			expected_interval: scheduled ? Math.max(1, Number(hours)) * 3600 : null,
			retention,
		});
		onDone();
	};
	const reset = async () => {
		await clearSchedule.call({ server_group_id: groupId, type: schedule.type });
		onDone();
	};

	const pending = setSchedule.pending || clearSchedule.pending;
	const error = setSchedule.error || clearSchedule.error;

	return (
		<Stack spacing={1.5} sx={{ mt: 1.5 }}>
			<FormControlLabel
				control={
					<Switch
						checked={scheduled}
						onChange={(e) => setScheduled(e.target.checked)}
						disabled={pending}
					/>
				}
				label={scheduled ? "Scheduled" : "Manual only"}
			/>
			{scheduled && (
				<TextField
					label="Back up every (hours)"
					type="number"
					size="small"
					value={hours}
					onChange={(e) => setHours(e.target.value)}
					disabled={pending}
					slotProps={{ htmlInput: { min: 1, step: 1 } }}
					sx={{ width: 200 }}
				/>
			)}
			<Stack direction={{ xs: "column", md: "row" }} spacing={1}>
				{RETENTION_FIELDS.map((f) => (
					<TextField
						key={f.key}
						label={f.label}
						type="number"
						size="small"
						value={retention[f.key]}
						onChange={(e) =>
							setRetention({ ...retention, [f.key]: Number(e.target.value) })
						}
						disabled={pending}
						error={f.floor != null && retention[f.key] < f.floor}
						helperText={f.floor != null ? `≥ ${f.floor}` : undefined}
						slotProps={{ htmlInput: { min: f.floor ?? 0, step: 1 } }}
						sx={{ width: 100 }}
					/>
				))}
			</Stack>
			{floorError.length > 0 && (
				<Alert severity="warning">{floorError.join("; ")}</Alert>
			)}
			{error && <Alert severity="error">{error.message}</Alert>}
			<Stack direction="row" spacing={1}>
				<Button
					variant="contained"
					size="small"
					onClick={save}
					disabled={pending || floorError.length > 0}
				>
					{pending ? "Saving…" : "Save override"}
				</Button>
				{schedule.has_override && (
					<Button
						size="small"
						color="warning"
						onClick={reset}
						disabled={pending}
					>
						Reset to default
					</Button>
				)}
				<Button size="small" onClick={onCancel} disabled={pending}>
					Cancel
				</Button>
			</Stack>
		</Stack>
	);
}

function ProvisioningCard({
	config,
	isAdmin,
	onRetried,
}: {
	config: BackupConfigView;
	isAdmin: boolean;
	onRetried: () => void;
}) {
	const retry = useApiAction("backups", "create_repo");
	const onRetry = async () => {
		try {
			await retry.call({ server_group_id: config.server_group_id });
			onRetried();
		} catch {
			/* surfaced via retry.error */
		}
	};
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			{config.last_init_error ? (
				<Stack spacing={2}>
					<Alert severity="error">
						Repository creation failed: {config.last_init_error}
					</Alert>
					{isAdmin && (
						<Box>
							<Button
								variant="contained"
								startIcon={<RefreshIcon />}
								onClick={onRetry}
								disabled={retry.pending}
							>
								{retry.pending ? "Retrying…" : "Retry repo creation"}
							</Button>
						</Box>
					)}
					{retry.error && (
						<Alert severity="error">{retry.error.message}</Alert>
					)}
				</Stack>
			) : (
				<Stack direction="row" spacing={2} sx={{ alignItems: "center" }}>
					<CircularProgress size={20} />
					<Typography>Creating repository…</Typography>
				</Stack>
			)}
		</Paper>
	);
}

function StatsPanel({ groupId }: { groupId: string }) {
	const stats = useApi(
		"backups",
		"stats",
		{ server_group_id: groupId },
		[groupId],
	);
	if (stats.status === "loading" || stats.status === "idle") {
		return <LinearProgress />;
	}
	if (stats.status === "error") {
		return <Alert severity="error">{stats.error.message}</Alert>;
	}
	const s = stats.data.stats;
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="h6" component="h2" gutterBottom>
				Repository stats
			</Typography>
			{s == null ? (
				<Typography color="text.secondary">
					No stats yet (awaiting first inspection).
				</Typography>
			) : (
				<Stack spacing={0.5}>
					<Stat label="Snapshots" value={s.snapshot_count} />
					<Stat label="Sources" value={s.source_count} />
					<Stat label="Logical bytes" value={formatBytes(s.logical_bytes)} />
					<Stat label="Physical bytes" value={formatBytes(s.physical_bytes)} />
					<Stat
						label="Bucket bytes"
						value={formatBytes(s.bucket_bytes)}
					/>
					<Typography variant="caption" color="text.secondary">
						Observed <TimeAgo timestamp={s.observed_at} />
					</Typography>
				</Stack>
			)}
			<Divider sx={{ my: 2 }} />
			<Typography variant="subtitle1" gutterBottom>
				Recent runs
			</Typography>
			{stats.data.recent_runs.length === 0 ? (
				<Typography color="text.secondary">No runs reported yet.</Typography>
			) : (
				<Table size="small">
					<TableHead>
						<TableRow>
							<TableCell>When</TableCell>
							<TableCell>Type</TableCell>
							<TableCell>Purpose</TableCell>
							<TableCell>Outcome</TableCell>
							<TableCell>Uploaded</TableCell>
						</TableRow>
					</TableHead>
					<TableBody>
						{stats.data.recent_runs.map((r) => (
							<TableRow key={r.id}>
								<TableCell>
									<TimeAgo timestamp={r.reported_at} />
								</TableCell>
								<TableCell>{r.type}</TableCell>
								<TableCell>{r.purpose}</TableCell>
								<TableCell>
									<Chip
										size="small"
										label={r.outcome}
										color={r.outcome === "success" ? "success" : "error"}
									/>
								</TableCell>
								<TableCell>{formatBytes(r.bytes_uploaded)}</TableCell>
							</TableRow>
						))}
					</TableBody>
				</Table>
			)}
		</Paper>
	);
}

function RunsAndRequests({
	groupId,
	members,
	isAdmin,
}: {
	groupId: string;
	members: ServerInfo[];
	isAdmin: boolean;
}) {
	const stats = useApi(
		"backups",
		"stats",
		{ server_group_id: groupId },
		[groupId],
	);
	const requestNow = useApiAction("backups", "request_now");
	const cancel = useApiAction("backups", "cancel_request");

	const pending =
		stats.status === "ok" ? stats.data.pending_requests : [];

	const isPending = (serverId: string) =>
		pending.find(
			(p) => p.server_id === serverId && p.purpose === "backup",
		);

	const onRequest = async (serverId: string) => {
		try {
			await requestNow.call({
				server_id: serverId,
				type: WELL_KNOWN_TYPE,
				purpose: "backup",
			});
			stats.reload();
		} catch {
			/* surfaced via requestNow.error */
		}
	};
	const onCancel = async (serverId: string) => {
		try {
			await cancel.call({
				server_id: serverId,
				type: WELL_KNOWN_TYPE,
				purpose: "backup",
			});
			stats.reload();
		} catch {
			/* surfaced via cancel.error */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="h6" component="h2" gutterBottom>
				Back up now
			</Typography>
			{members.length === 0 ? (
				<Typography color="text.secondary">
					No member servers in this group.
				</Typography>
			) : (
				<Stack spacing={1} divider={<Divider />}>
					{members.map((m) => {
						const req = isPending(m.id);
						return (
							<Stack
								key={m.id}
								direction="row"
								spacing={2}
								sx={{ alignItems: "center", justifyContent: "space-between" }}
							>
								<Typography variant="body2">
									{m.name ?? m.id.slice(0, 8)}
								</Typography>
								{req ? (
									<Stack
										direction="row"
										spacing={1}
										sx={{ alignItems: "center" }}
									>
										<Chip
											size="small"
											color="info"
											label={
												<>
													requested <TimeAgo timestamp={req.requested_at} />
												</>
											}
										/>
										{isAdmin && (
											<Button
												size="small"
												color="error"
												onClick={() => onCancel(m.id)}
												disabled={cancel.pending}
											>
												Cancel
											</Button>
										)}
									</Stack>
								) : (
									isAdmin && (
										<Button
											size="small"
											variant="outlined"
											startIcon={<BackupIcon />}
											onClick={() => onRequest(m.id)}
											disabled={requestNow.pending}
										>
											Backup now
										</Button>
									)
								)}
							</Stack>
						);
					})}
				</Stack>
			)}
			{(requestNow.error || cancel.error) && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{(requestNow.error || cancel.error)!.message}
				</Alert>
			)}
		</Paper>
	);
}

function Stat({
	label,
	value,
}: {
	label: string;
	value: number | string | null;
}) {
	return (
		<Typography variant="body2">
			<strong>{label}:</strong>{" "}
			{value == null ? "unknown" : value}
		</Typography>
	);
}

/// Format a byte count, rendering null as "unknown" (indicators always show a
/// state, never hide).
function formatBytes(bytes: number | null): string {
	if (bytes == null) return "unknown";
	if (bytes < 1024) return `${bytes} B`;
	const units = ["KiB", "MiB", "GiB", "TiB"];
	let v = bytes / 1024;
	let i = 0;
	while (v >= 1024 && i < units.length - 1) {
		v /= 1024;
		i++;
	}
	return `${v.toFixed(1)} ${units[i]}`;
}
