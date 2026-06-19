import {
	Alert,
	Box,
	Button,
	Chip,
	CircularProgress,
	Divider,
	LinearProgress,
	Paper,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableRow,
	Typography,
} from "@mui/material";
import BackupIcon from "@mui/icons-material/Backup";
import EditIcon from "@mui/icons-material/Edit";
import RefreshIcon from "@mui/icons-material/Refresh";
import { Link as RouterLink, useParams } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { humanSeconds } from "../lib/humanDuration";
import { usePageTitle } from "../hooks/usePageTitle";
import TimeAgo from "../components/TimeAgo";
import BackupEscrow from "./BackupEscrow";
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
					<Button
						component={RouterLink}
						to={`/groups/${id}/backups/config`}
						variant="outlined"
						startIcon={<EditIcon />}
					>
						Edit config
					</Button>
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

			{status === "escrow_pending" && (
				<BackupEscrow config={data} onAcked={configForTick.reload} />
			)}

			{status === "ready" && (
				<>
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

function ConfigSummary({ config }: { config: BackupConfigView }) {
	const sched = config.schedules.find((s) => s.type === WELL_KNOWN_TYPE);
	const interval = sched?.expected_interval;
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
					<strong>Schedule:</strong>{" "}
					{interval != null
						? `every ${humanSeconds(interval)}`
						: "Manual only"}
				</Typography>
				{sched?.retention && (
					<Typography variant="body2">
						<strong>Retention:</strong> latest {sched.retention.keep_latest},
						daily {sched.retention.keep_daily}, weekly{" "}
						{sched.retention.keep_weekly}, monthly{" "}
						{sched.retention.keep_monthly}, annual{" "}
						{sched.retention.keep_annual}
					</Typography>
				)}
			</Stack>
		</Paper>
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
