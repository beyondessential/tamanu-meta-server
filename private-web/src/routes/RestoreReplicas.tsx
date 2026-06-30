import AddIcon from "@mui/icons-material/Add";
import DeleteIcon from "@mui/icons-material/Delete";
import {
	Alert,
	Box,
	Button,
	Chip,
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
import { useState } from "react";
import { ApiError, callApi, useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

const WELL_KNOWN_INTENTS = ["verify", "analytics", "disaster-recovery"];

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

export default function RestoreReplicas() {
	usePageTitle("Restore replicas");
	const [tick, setTick] = useState(0);
	const reload = () => setTick((t) => t + 1);

	const replicas = useApi("restore_replicas", "list", {}, [tick]);
	const consumers = useApi("restore_replicas", "consumers", {}, [tick]);
	const checks = useApi("restore_replicas", "checks", {}, [tick]);

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
		<Stack spacing={3}>
			<Stack
				direction="row"
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h5" component="h1">
					Restore replicas
				</Typography>
				<Button
					variant="contained"
					startIcon={<AddIcon />}
					onClick={() => setCreateOpen(true)}
				>
					Declare replica
				</Button>
			</Stack>

			<Typography variant="body2" color="text.secondary">
				Canopy decides which replicas a restore consumer should keep. Each
				declaration expands to one replica per matching server, restored from
				the latest snapshot Canopy knows about.
			</Typography>

			{error && (
				<Alert severity="error" onClose={() => setError(null)}>
					{error}
				</Alert>
			)}

			<Box>
				<Typography variant="h6" component="h2" gutterBottom>
					Declarations
				</Typography>
				{replicas.status === "loading" || replicas.status === "idle" ? (
					<LinearProgress />
				) : replicas.status === "error" ? (
					<Alert severity="error">{replicas.error.message}</Alert>
				) : replicas.data.length === 0 ? (
					<Alert severity="info">No restore replicas declared.</Alert>
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
									<TableCell align="right">Actions</TableCell>
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
												onChange={(e) =>
													onToggle(
														r.id,
														r.name,
														r.freshness_seconds,
														e.target.checked,
													)
												}
												slotProps={{
													input: { "aria-label": `toggle ${r.name}` },
												}}
											/>
										</TableCell>
										<TableCell align="right">
											<IconButton
												edge="end"
												aria-label={`delete ${r.name}`}
												onClick={() => onDelete(r.id)}
											>
												<DeleteIcon />
											</IconButton>
										</TableCell>
									</TableRow>
								))}
							</TableBody>
						</Table>
					</Paper>
				)}
			</Box>

			<Box>
				<Typography variant="h6" component="h2" gutterBottom>
					Consumers
				</Typography>
				{consumers.status === "ok" && consumers.data.length === 0 && (
					<Alert severity="info">
						No restore consumers. Promote a device to the{" "}
						<code>backup-restore</code> role on its device page.
					</Alert>
				)}
				{consumers.status === "ok" && consumers.data.length > 0 && (
					<Stack spacing={1}>
						{consumers.data.map((c) => (
							<Paper key={c.device_id} variant="outlined" sx={{ p: 1.5 }}>
								<Typography variant="subtitle2">
									{c.name ?? c.device_id}
								</Typography>
								<Stack direction="row" spacing={0.5} sx={{ mt: 0.5 }}>
									{c.intents.length === 0 ? (
										<Typography variant="body2" color="text.secondary">
											No capabilities registered yet.
										</Typography>
									) : (
										c.intents.map((i) => (
											<Chip key={i} label={i} size="small" />
										))
									)}
								</Stack>
							</Paper>
						))}
					</Stack>
				)}
			</Box>

			<Box>
				<Typography variant="h6" component="h2" gutterBottom>
					Recent restore checks
				</Typography>
				{checks.status === "ok" && checks.data.length === 0 ? (
					<Alert severity="info">No restore-health reports yet.</Alert>
				) : checks.status === "ok" ? (
					<Paper variant="outlined">
						<Table size="small">
							<TableHead>
								<TableRow>
									<TableCell>When</TableCell>
									<TableCell>Server</TableCell>
									<TableCell>Type</TableCell>
									<TableCell>Intent</TableCell>
									<TableCell>Outcome</TableCell>
									<TableCell>Snapshot</TableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{checks.data.map((c) => {
									const ok = c.outcome === "success" && c.replica_healthy;
									return (
										<TableRow key={c.id}>
											<TableCell>
												{new Date(c.observed_at).toLocaleString()}
											</TableCell>
											<TableCell>
												{c.server_id ? c.server_id.slice(0, 8) : "—"}
											</TableCell>
											<TableCell>{c.type}</TableCell>
											<TableCell>{c.intent}</TableCell>
											<TableCell>
												<Chip
													label={ok ? "healthy" : "failed"}
													color={ok ? "success" : "error"}
													size="small"
												/>
											</TableCell>
											<TableCell>
												{c.snapshot_id ? c.snapshot_id.slice(0, 12) : "—"}
											</TableCell>
										</TableRow>
									);
								})}
							</TableBody>
						</Table>
					</Paper>
				) : null}
			</Box>

			{createOpen && (
				<CreateReplicaDialog
					onClose={() => setCreateOpen(false)}
					onCreated={() => {
						setCreateOpen(false);
						reload();
					}}
					consumers={
						consumers.status === "ok" ? consumers.data : []
					}
				/>
			)}
		</Stack>
	);
}

interface ConsumerOption {
	device_id: string;
	name?: string | null;
	intents: string[];
}

function CreateReplicaDialog({
	onClose,
	onCreated,
	consumers,
}: {
	onClose: () => void;
	onCreated: () => void;
	consumers: ConsumerOption[];
}) {
	const groups = useApi("server_groups", "list");
	const typeDefaults = useApi("backups", "type_defaults");

	const [consumerId, setConsumerId] = useState("");
	const [groupId, setGroupId] = useState("");
	const [serverId, setServerId] = useState(""); // "" = whole group
	const [type, setType] = useState("tamanu-postgres");
	const [intent, setIntent] = useState("verify");
	const [name, setName] = useState("");
	const [freshnessHours, setFreshnessHours] = useState("");
	const [pending, setPending] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const selectedConsumer = consumers.find((c) => c.device_id === consumerId);
	const intentOptions = Array.from(
		new Set([...(selectedConsumer?.intents ?? []), ...WELL_KNOWN_INTENTS]),
	);

	const onSubmit = async () => {
		if (!consumerId) return setError("Pick a consumer");
		if (!groupId) return setError("Pick a group");
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

	const typeOptions =
		typeDefaults.status === "ok" && typeDefaults.data.length > 0
			? typeDefaults.data.map((t) => t.type)
			: ["tamanu-postgres"];

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
						<InputLabel id="group-label">Group</InputLabel>
						<Select
							labelId="group-label"
							label="Group"
							value={groupId}
							onChange={(e) => {
								setGroupId(e.target.value);
								setServerId("");
							}}
						>
							{groups.status === "ok" &&
								groups.data.map((g) => (
									<MenuItem key={g.id} value={g.id}>
										{g.name ?? g.id}
									</MenuItem>
								))}
						</Select>
					</FormControl>

					{groupId && (
						<ServerScopeSelect
							groupId={groupId}
							value={serverId}
							onChange={setServerId}
						/>
					)}

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
							{intentOptions.map((i) => {
								const supported =
									selectedConsumer?.intents.includes(i) ?? false;
								return (
									<MenuItem key={i} value={i}>
										{i}
										{!supported && " (unsupported — will be a gap)"}
									</MenuItem>
								);
							})}
						</Select>
					</FormControl>

					<TextField
						size="small"
						fullWidth
						label="Name"
						value={name}
						onChange={(e) => setName(e.target.value)}
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

function ServerScopeSelect({
	groupId,
	value,
	onChange,
}: {
	groupId: string;
	value: string;
	onChange: (v: string) => void;
}) {
	const detail = useApi(
		"server_groups",
		"get",
		{ server_group_id: groupId },
		[groupId],
	);
	const servers =
		detail.status === "ok" ? detail.data.servers.filter((s) => !s.archived) : [];
	return (
		<FormControl fullWidth size="small">
			<InputLabel id="server-label">Server</InputLabel>
			<Select
				labelId="server-label"
				label="Server"
				value={value}
				onChange={(e) => onChange(e.target.value)}
			>
				<MenuItem value="">All servers in the group</MenuItem>
				{servers.map((s) => (
					<MenuItem key={s.id} value={s.id}>
						{s.name ?? s.display_host ?? s.id}
					</MenuItem>
				))}
			</Select>
		</FormControl>
	);
}
