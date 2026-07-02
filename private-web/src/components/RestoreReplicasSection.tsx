import AddIcon from "@mui/icons-material/Add";
import DeleteIcon from "@mui/icons-material/Delete";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
	Alert,
	Box,
	Button,
	Chip,
	Collapse,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	FormControl,
	IconButton,
	InputLabel,
	LinearProgress,
	MenuItem,
	Paper,
	Select,
	Stack,
	Switch,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableRow,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import { useEffect, useState } from "react";
import { ApiError, callApi, useApi } from "../api";
import type { BackupRestoreCheck } from "../types";

function kebabCase(s: string): string {
	return s
		.trim()
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-+|-+$/g, "");
}

function formatError(err: unknown): string {
	if (err instanceof ApiError) {
		const detail = err.detail as { title?: string } | null;
		return detail?.title ?? err.message;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

function freshnessLabel(seconds: number | null | undefined): string {
	if (seconds == null) return "latest";
	const hours = seconds / 3600;
	return hours >= 1 ? `${hours}h` : `${seconds}s`;
}

interface ConsumerOption {
	device_id: string;
	name?: string | null;
	intents: string[];
}

/** Managed restore replicas for one group — the declarations Canopy drives and
 * the restore-health reports it has received back, shown on the group's backup
 * page. */
export default function RestoreReplicasSection({
	groupId,
	isAdmin,
}: {
	groupId: string;
	isAdmin: boolean;
}) {
	const [tick, setTick] = useState(0);
	const reload = () => setTick((t) => t + 1);

	const replicas = useApi(
		"restore_replicas",
		"for_group",
		{ server_group_id: groupId },
		[groupId, tick],
	);
	const consumers = useApi("restore_replicas", "consumers", {}, [tick]);
	const checks = useApi(
		"restore_replicas",
		"checks",
		{ server_group_id: groupId },
		[groupId, tick],
	);

	const [createOpen, setCreateOpen] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const onDelete = async (id: string) => {
		try {
			await callApi("restore_replicas", "delete", { id });
			reload();
		} catch (err) {
			setError(formatError(err));
		}
	};

	const onToggle = async (
		id: string,
		name: string,
		freshnessSeconds: number | null | undefined,
		enabled: boolean,
	) => {
		try {
			await callApi("restore_replicas", "update", {
				id,
				name,
				freshness_seconds: freshnessSeconds ?? null,
				enabled,
			});
			reload();
		} catch (err) {
			setError(formatError(err));
		}
	};

	return (
		<Box>
			<Stack
				direction="row"
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="h6" component="h2">
					Restore replicas
				</Typography>
				{isAdmin && (
					<Button
						size="small"
						variant="outlined"
						startIcon={<AddIcon />}
						onClick={() => setCreateOpen(true)}
					>
						Declare replica
					</Button>
				)}
			</Stack>

			<Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
				Canopy drives a restore consumer to keep these replicas, each restored
				from the latest snapshot for its server.
			</Typography>

			{error && (
				<Alert severity="error" sx={{ mb: 1 }} onClose={() => setError(null)}>
					{error}
				</Alert>
			)}

			{replicas.status === "loading" || replicas.status === "idle" ? (
				<LinearProgress />
			) : replicas.status === "error" ? (
				<Alert severity="error">{replicas.error.message}</Alert>
			) : replicas.data.length === 0 ? (
				<Alert severity="info">No restore replicas declared for this group.</Alert>
			) : (
				<Paper variant="outlined">
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell>Name</TableCell>
								<TableCell>Consumer</TableCell>
								<TableCell>Scope</TableCell>
								<TableCell>Type</TableCell>
								<TableCell>Intent</TableCell>
								<TableCell>Freshness</TableCell>
								<TableCell>Enabled</TableCell>
								{isAdmin && <TableCell align="right">Actions</TableCell>}
							</TableRow>
						</TableHead>
						<TableBody>
							{replicas.data.map((r) => (
								<TableRow key={r.id}>
									<TableCell>{r.name}</TableCell>
									<TableCell>
										{r.consumer_name ?? r.consumer_device_id.slice(0, 8)}
									</TableCell>
									<TableCell>
										{r.server_id ? "one server" : "whole group"}
									</TableCell>
									<TableCell>{r.type}</TableCell>
									<TableCell>
										<Stack
											direction="row"
											spacing={0.5}
											sx={{ alignItems: "center" }}
										>
											<span>{r.intent}</span>
											{r.gap && (
												<Tooltip title="The consumer does not currently support this intent, so Canopy is not dispatching it.">
													<Chip label="gap" color="warning" size="small" />
												</Tooltip>
											)}
										</Stack>
									</TableCell>
									<TableCell>{freshnessLabel(r.freshness_seconds)}</TableCell>
									<TableCell>
										<Switch
											checked={r.enabled}
											disabled={!isAdmin}
											onChange={(e) =>
												onToggle(r.id, r.name, r.freshness_seconds, e.target.checked)
											}
											slotProps={{ input: { "aria-label": `toggle ${r.name}` } }}
										/>
									</TableCell>
									{isAdmin && (
										<TableCell align="right">
											<IconButton
												edge="end"
												aria-label={`delete ${r.name}`}
												onClick={() => onDelete(r.id)}
											>
												<DeleteIcon />
											</IconButton>
										</TableCell>
									)}
								</TableRow>
							))}
						</TableBody>
					</Table>
				</Paper>
			)}

			<Typography variant="subtitle2" sx={{ mt: 2, mb: 1 }}>
				Recent restore checks
			</Typography>
			{checks.status === "ok" && checks.data.length === 0 ? (
				<Alert severity="info">No restore-health reports yet.</Alert>
			) : checks.status === "ok" ? (
				<Paper variant="outlined">
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell padding="checkbox" />
								<TableCell>When</TableCell>
								<TableCell>Server</TableCell>
								<TableCell>Type</TableCell>
								<TableCell>Intent</TableCell>
								<TableCell>Outcome</TableCell>
								<TableCell>PG version</TableCell>
								<TableCell>Snapshot</TableCell>
							</TableRow>
						</TableHead>
						<TableBody>
							{checks.data.map((c) => (
								<CheckRow key={c.id} check={c} />
							))}
						</TableBody>
					</Table>
				</Paper>
			) : null}

			{createOpen && (
				<CreateReplicaDialog
					groupId={groupId}
					onClose={() => setCreateOpen(false)}
					onCreated={() => {
						setCreateOpen(false);
						reload();
					}}
					consumers={consumers.status === "ok" ? consumers.data : []}
				/>
			)}
		</Box>
	);
}

function CreateReplicaDialog({
	groupId,
	onClose,
	onCreated,
	consumers,
}: {
	groupId: string;
	onClose: () => void;
	onCreated: () => void;
	consumers: ConsumerOption[];
}) {
	const detail = useApi(
		"server_groups",
		"get",
		{ server_group_id: groupId },
		[groupId],
	);
	const typeDefaults = useApi("backups", "type_defaults");

	const [consumerId, setConsumerId] = useState("");
	const [serverId, setServerId] = useState(""); // "" = whole group
	const [type, setType] = useState("tamanu-postgres");
	const [intent, setIntent] = useState("");
	const [name, setName] = useState("");
	const [nameEdited, setNameEdited] = useState(false);
	const [freshnessHours, setFreshnessHours] = useState("");
	const [pending, setPending] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const selectedConsumer = consumers.find((c) => c.device_id === consumerId);
	const intentOptions = selectedConsumer?.intents ?? [];
	const servers =
		detail.status === "ok"
			? detail.data.servers.filter((s) => !s.archived)
			: [];
	const typeOptions =
		typeDefaults.status === "ok" && typeDefaults.data.length > 0
			? typeDefaults.data.map((t) => t.type)
			: ["tamanu-postgres"];

	// Auto-select the sole consumer, if there's only one to choose from.
	useEffect(() => {
		if (!consumerId && consumers.length === 1) {
			setConsumerId(consumers[0].device_id);
		}
	}, [consumers, consumerId]);

	// Keep the intent on a value the selected consumer actually supports.
	useEffect(() => {
		if (intentOptions.length > 0 && !intentOptions.includes(intent)) {
			setIntent(intentOptions[0]);
		}
	}, [intentOptions, intent]);

	// Suggest a name from the group and (if picked) server, until the operator
	// types their own.
	const groupName = detail.status === "ok" ? detail.data.group.name : "";
	const selectedServer = servers.find((s) => s.id === serverId);
	const serverName = selectedServer
		? (selectedServer.name ?? selectedServer.display_host ?? selectedServer.id)
		: "";
	const suggestedName = kebabCase(
		[groupName, serverName].filter(Boolean).join("-"),
	);
	useEffect(() => {
		if (!nameEdited) setName(suggestedName);
	}, [suggestedName, nameEdited]);

	const onSubmit = async () => {
		if (!consumerId) return setError("Pick a consumer");
		if (!intent) return setError("Pick an intent the consumer supports");
		if (!name.trim()) return setError("Name cannot be empty");
		const hours = freshnessHours.trim();
		const freshness_seconds =
			hours === "" ? null : Math.round(Number(hours) * 3600);
		if (freshness_seconds != null && !Number.isFinite(freshness_seconds)) {
			return setError("Freshness must be a number of hours");
		}
		setPending(true);
		setError(null);
		try {
			await callApi("restore_replicas", "create", {
				consumer_device_id: consumerId,
				group_id: groupId,
				server_id: serverId || null,
				type,
				intent,
				name: name.trim(),
				freshness_seconds,
			});
			onCreated();
		} catch (err) {
			setError(formatError(err));
			setPending(false);
		}
	};

	return (
		<Dialog open onClose={() => !pending && onClose()} fullWidth maxWidth="sm">
			<DialogTitle>Declare restore replica</DialogTitle>
			<DialogContent>
				<Stack spacing={2} sx={{ mt: 1 }}>
					<FormControl fullWidth size="small">
						<InputLabel id="consumer-label">Consumer</InputLabel>
						<Select
							labelId="consumer-label"
							label="Consumer"
							value={consumerId}
							onChange={(e) => {
								setConsumerId(e.target.value);
								setError(null);
							}}
						>
							{consumers.map((c) => (
								<MenuItem key={c.device_id} value={c.device_id}>
									{c.name ?? c.device_id}
								</MenuItem>
							))}
						</Select>
					</FormControl>

					<FormControl fullWidth size="small">
						<InputLabel id="server-label">Server</InputLabel>
						<Select
							labelId="server-label"
							label="Server"
							value={serverId}
							onChange={(e) => setServerId(e.target.value)}
						>
							<MenuItem value="">All servers in the group</MenuItem>
							{servers.map((s) => (
								<MenuItem key={s.id} value={s.id}>
									{s.name ?? s.display_host ?? s.id}
								</MenuItem>
							))}
						</Select>
					</FormControl>

					<FormControl fullWidth size="small">
						<InputLabel id="type-label">Type</InputLabel>
						<Select
							labelId="type-label"
							label="Type"
							value={type}
							onChange={(e) => setType(e.target.value)}
						>
							{typeOptions.map((t) => (
								<MenuItem key={t} value={t}>
									{t}
								</MenuItem>
							))}
						</Select>
					</FormControl>

					<FormControl fullWidth size="small">
						<InputLabel id="intent-label">Intent</InputLabel>
						<Select
							labelId="intent-label"
							label="Intent"
							value={intent}
							onChange={(e) => setIntent(e.target.value)}
						>
							{intentOptions.map((i) => (
								<MenuItem key={i} value={i}>
									{i}
								</MenuItem>
							))}
						</Select>
					</FormControl>

					<TextField
						size="small"
						fullWidth
						label="Name"
						value={name}
						onChange={(e) => {
							setName(e.target.value);
							setNameEdited(true);
						}}
					/>

					<TextField
						size="small"
						fullWidth
						type="number"
						label="Freshness (hours, optional)"
						placeholder="latest only"
						value={freshnessHours}
						onChange={(e) => setFreshnessHours(e.target.value)}
					/>

					{error && <Alert severity="error">{error}</Alert>}
				</Stack>
			</DialogContent>
			<DialogActions>
				<Button onClick={onClose} disabled={pending}>
					Cancel
				</Button>
				<Button variant="contained" onClick={onSubmit} disabled={pending}>
					{pending ? "Declaring…" : "Declare"}
				</Button>
			</DialogActions>
		</Dialog>
	);
}

/** One restore-check row. When the consumer sent arbitrary `health_details`,
 * the row expands to reveal it as pretty-printed JSON. */
function CheckRow({ check }: { check: BackupRestoreCheck }) {
	const [open, setOpen] = useState(false);
	const ok = check.outcome === "success" && check.replica_healthy;
	const hasDetails =
		check.health_details != null &&
		!(
			typeof check.health_details === "object" &&
			Object.keys(check.health_details).length === 0
		);
	return (
		<>
			<TableRow sx={hasDetails ? { "& > *": { borderBottom: "unset" } } : undefined}>
				<TableCell padding="checkbox">
					{hasDetails && (
						<IconButton
							size="small"
							aria-label={open ? "Hide health details" : "Show health details"}
							onClick={() => setOpen((o) => !o)}
						>
							{open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
						</IconButton>
					)}
				</TableCell>
				<TableCell>{new Date(check.observed_at).toLocaleString()}</TableCell>
				<TableCell>{check.server_id ? check.server_id.slice(0, 8) : "—"}</TableCell>
				<TableCell>{check.type}</TableCell>
				<TableCell>{check.intent}</TableCell>
				<TableCell>
					<Chip
						label={ok ? "healthy" : "failed"}
						color={ok ? "success" : "error"}
						size="small"
					/>
				</TableCell>
				<TableCell>{check.postgres_version ?? "—"}</TableCell>
				<TableCell>
					{check.snapshot_id ? check.snapshot_id.slice(0, 12) : "—"}
				</TableCell>
			</TableRow>
			{hasDetails && (
				<TableRow>
					<TableCell sx={{ py: 0 }} colSpan={8}>
						<Collapse in={open} timeout="auto" unmountOnExit>
							<Box sx={{ my: 1 }}>
								<Typography variant="caption" color="text.secondary">
									Health details
								</Typography>
								<Box
									component="pre"
									sx={{
										m: 0,
										mt: 0.5,
										p: 1,
										fontSize: "0.75rem",
										overflowX: "auto",
										bgcolor: "action.hover",
										borderRadius: 1,
									}}
								>
									{JSON.stringify(check.health_details, null, 2)}
								</Box>
							</Box>
						</Collapse>
					</TableCell>
				</TableRow>
			)}
		</>
	);
}
