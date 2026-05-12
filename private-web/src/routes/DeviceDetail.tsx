import {
	Alert,
	Box,
	Button,
	Chip,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	IconButton,
	LinearProgress,
	MenuItem,
	Paper,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
import EditIcon from "@mui/icons-material/Edit";
import RefreshIcon from "@mui/icons-material/Refresh";
import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import ServerShorty, {
	type ServerInfo as ServerShortyInfo,
} from "../components/ServerShorty";
import { type ApiState, callApi, useApi, useApiAction } from "../api";
import { deviceDisplayName } from "../components/DeviceShorty";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";
import { humanDuration } from "../lib/humanDuration";
import type {
	DeviceConnectionData,
	DeviceInfo,
	DeviceKeyInfo,
	DeviceRole,
	TailnetLiveInfo,
} from "../types";

const TRUSTABLE_ROLES: DeviceRole[] = ["server", "releaser", "admin"];

export default function DeviceDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const [tick, setTick] = useState(0);
	const detail = useApi(
		"devices",
		"get_device_by_id",
		{ device_id: id },
		[id, tick],
	);
	const refresh = () => setTick((t) => t + 1);
	usePageTitle(
		detail.status === "ok"
			? `Device ${deviceDisplayName(detail.data)}`
			: "Device",
	);

	if (detail.status === "loading" || detail.status === "idle") return <LinearProgress />;
	if (detail.status === "error")
		return <Alert severity="error">{detail.error.message}</Alert>;

	return <DeviceView device={detail.data} refresh={refresh} />;
}

function DeviceView({
	device,
	refresh,
}: {
	device: DeviceInfo;
	refresh: () => void;
}) {
	const name = deviceDisplayName(device);
	const role = device.device.role;
	return (
		<Stack spacing={3}>
			<Typography variant="h4" component="h1">
				Device {name}
			</Typography>
			<DeviceInfoBox device={device} />
			<TailnetIdentitySection device={device} refresh={refresh} />
			<KeysBox device={device} refresh={refresh} />
			<RoleControls device={device} refresh={refresh} />
			{role !== "untrusted" && (
				<AssociatedServersSection deviceId={device.device.id} />
			)}
			<PastServersSection deviceId={device.device.id} />
			<ConnectionHistory deviceId={device.device.id} />
		</Stack>
	);
}

function TailnetIdentitySection({
	device,
	refresh,
}: {
	device: DeviceInfo;
	refresh: () => void;
}) {
	const [attachOpen, setAttachOpen] = useState(false);
	const [mergeOpen, setMergeOpen] = useState(false);
	const [confirmDetach, setConfirmDetach] = useState(false);
	const detachAction = useApiAction("devices", "detach_tailscale");

	const nodeId = device.device.tailscale_node_id;
	const live = device.tailnet_live;

	const onDetach = async () => {
		try {
			await detachAction.call({ device_id: device.device.id });
			setConfirmDetach(false);
			refresh();
		} catch {
			/* surfaced via detachAction.error */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack spacing={1.5}>
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center", justifyContent: "space-between" }}
				>
					<Typography variant="h6" component="h2">
						Tailscale identity
					</Typography>
					{nodeId ? (
						<Chip
							size="small"
							label="Attached"
							color="success"
							variant="outlined"
						/>
					) : (
						<Chip
							size="small"
							label="Unknown"
							variant="outlined"
						/>
					)}
				</Stack>

				{nodeId ? (
					<TailnetAttachedView device={device} live={live} />
				) : (
					<Typography variant="body2" color="text.secondary">
						This device has no Tailscale identity attached. Attach
						one to let the device authenticate over the tailnet
						without an mTLS cert.
					</Typography>
				)}

				<Stack direction="row" spacing={1} useFlexGap>
					<Button
						variant="contained"
						onClick={() => setAttachOpen(true)}
					>
						{nodeId ? "Replace identity" : "Attach Tailscale identity"}
					</Button>
					{nodeId &&
						(confirmDetach ? (
							<Stack direction="row" spacing={1}>
								<Button
									variant="contained"
									color="error"
									onClick={onDetach}
									disabled={detachAction.pending}
								>
									{detachAction.pending
										? "Detaching…"
										: "Confirm detach"}
								</Button>
								<Button
									variant="outlined"
									onClick={() => setConfirmDetach(false)}
									disabled={detachAction.pending}
								>
									Cancel
								</Button>
							</Stack>
						) : (
							<Button
								variant="outlined"
								color="error"
								onClick={() => setConfirmDetach(true)}
							>
								Detach
							</Button>
						))}
					{device.device.role === "untrusted" && (
						<Button
							variant="outlined"
							onClick={() => setMergeOpen(true)}
						>
							Merge into existing device…
						</Button>
					)}
				</Stack>

				{detachAction.error && (
					<Alert severity="error">{detachAction.error.message}</Alert>
				)}
			</Stack>

			<AttachTailscaleDialog
				open={attachOpen}
				onClose={() => setAttachOpen(false)}
				deviceId={device.device.id}
				onAttached={() => {
					setAttachOpen(false);
					refresh();
				}}
			/>
			<MergeIntoDialog
				open={mergeOpen}
				onClose={() => setMergeOpen(false)}
				sourceId={device.device.id}
				onMerged={() => {
					setMergeOpen(false);
					refresh();
				}}
			/>
		</Paper>
	);
}

function TailnetAttachedView({
	device,
	live,
}: {
	device: DeviceInfo;
	live: TailnetLiveInfo | null;
}) {
	const items: Array<{ label: string; value: React.ReactNode; mono?: boolean }> = [];
	items.push({
		label: "Node ID",
		value: device.device.tailscale_node_id ?? "—",
		mono: true,
	});
	items.push({
		label: "Stored name",
		value: device.device.tailscale_node_name ?? "—",
	});
	items.push({
		label: "Tailnet",
		value: device.device.tailscale_tailnet ?? "—",
	});
	if (live) {
		items.push({
			label: "Current name",
			value: live.display_name,
		});
		items.push({
			label: "Current IPs",
			value: live.addresses.join(", "),
			mono: true,
		});
		if (live.tags.length > 0) {
			items.push({
				label: "Tags",
				value: live.tags.join(", "),
				mono: true,
			});
		}
	} else if (device.device.tailscale_node_id) {
		items.push({
			label: "Current",
			value: "Not in directory cache (node may have left the tailnet)",
		});
	}

	return (
		<Stack
			direction="row"
			spacing={4}
			useFlexGap
			sx={{ flexWrap: "wrap" }}
		>
			{items.map(({ label, value, mono }) => (
				<Stack key={label} spacing={0.25}>
					<Typography variant="caption" color="text.secondary">
						{label}
					</Typography>
					<Typography
						variant="body2"
						component="div"
						sx={mono ? { fontFamily: "monospace" } : undefined}
					>
						{value}
					</Typography>
				</Stack>
			))}
		</Stack>
	);
}

function AttachTailscaleDialog({
	open,
	onClose,
	deviceId,
	onAttached,
}: {
	open: boolean;
	onClose: () => void;
	deviceId: string;
	onAttached: () => void;
}) {
	const [identifier, setIdentifier] = useState("");
	const [preview, setPreview] = useState<TailnetLiveInfo | null>(null);
	const [previewError, setPreviewError] = useState<string | null>(null);
	const [previewLoading, setPreviewLoading] = useState(false);
	const attachAction = useApiAction("devices", "attach_tailscale");

	useEffect(() => {
		if (!open) {
			setIdentifier("");
			setPreview(null);
			setPreviewError(null);
		}
	}, [open]);

	useEffect(() => {
		const value = identifier.trim();
		if (!open || value === "") {
			setPreview(null);
			setPreviewError(null);
			return;
		}
		let cancelled = false;
		setPreviewLoading(true);
		const handle = setTimeout(async () => {
			try {
				const r = await callApi(
					"devices",
					"resolve_tailnet_identifier",
					{ identifier: value },
				);
				if (cancelled) return;
				setPreview(r.matched);
				setPreviewError(
					r.matched ? null : "No tailnet node matches that identifier.",
				);
			} catch (err) {
				if (cancelled) return;
				setPreview(null);
				setPreviewError(err instanceof Error ? err.message : String(err));
			} finally {
				if (!cancelled) setPreviewLoading(false);
			}
		}, 250);
		return () => {
			cancelled = true;
			clearTimeout(handle);
		};
	}, [identifier, open]);

	const onConfirm = async () => {
		try {
			await attachAction.call({ device_id: deviceId, identifier });
			onAttached();
		} catch {
			/* surfaced via attachAction.error */
		}
	};

	return (
		<Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
			<DialogTitle>Attach Tailscale identity</DialogTitle>
			<DialogContent>
				<Stack spacing={2} sx={{ mt: 1 }}>
					<TextField
						label="Tailscale identifier"
						placeholder="100.64.0.42 / nodekey:… / device.example.ts.net"
						helperText="Paste a Tailscale IP, node id, or DNS name from the admin console."
						value={identifier}
						onChange={(e) => setIdentifier(e.target.value)}
						autoFocus
						fullWidth
					/>
					{previewLoading && <LinearProgress />}
					{preview && (
						<Paper variant="outlined" sx={{ p: 1.5 }}>
							<Stack spacing={0.5}>
								<Typography variant="caption" color="text.secondary">
									Resolves to
								</Typography>
								<Typography variant="body2">
									{preview.display_name}
								</Typography>
								<Typography
									variant="body2"
									color="text.secondary"
									sx={{ fontFamily: "monospace" }}
								>
									{preview.node_id}
								</Typography>
								<Typography variant="body2" color="text.secondary">
									{preview.addresses.join(", ")}
								</Typography>
								{preview.tags.length > 0 && (
									<Typography
										variant="body2"
										color="text.secondary"
									>
										Tags: {preview.tags.join(", ")}
									</Typography>
								)}
							</Stack>
						</Paper>
					)}
					{previewError && identifier.trim() !== "" && (
						<Alert severity="info">{previewError}</Alert>
					)}
					{attachAction.error && (
						<Alert severity="error">
							{attachAction.error.message}
						</Alert>
					)}
				</Stack>
			</DialogContent>
			<DialogActions>
				<Button onClick={onClose} disabled={attachAction.pending}>
					Cancel
				</Button>
				<Button
					variant="contained"
					onClick={onConfirm}
					disabled={
						attachAction.pending ||
						preview === null ||
						identifier.trim() === ""
					}
				>
					{attachAction.pending ? "Attaching…" : "Attach"}
				</Button>
			</DialogActions>
		</Dialog>
	);
}

function MergeIntoDialog({
	open,
	onClose,
	sourceId,
	onMerged,
}: {
	open: boolean;
	onClose: () => void;
	sourceId: string;
	onMerged: () => void;
}) {
	const [targetId, setTargetId] = useState("");
	const mergeAction = useApiAction("devices", "merge_into");

	useEffect(() => {
		if (!open) {
			setTargetId("");
		}
	}, [open]);

	const onConfirm = async () => {
		try {
			await mergeAction.call({ source_id: sourceId, target_id: targetId });
			onMerged();
		} catch {
			/* surfaced via mergeAction.error */
		}
	};

	return (
		<Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
			<DialogTitle>Merge into existing device</DialogTitle>
			<DialogContent>
				<Stack spacing={2} sx={{ mt: 1 }}>
					<Typography variant="body2" color="text.secondary">
						The current device row will be folded into the target.
						The target keeps its role, server attachment, and history;
						this device's Tailscale identity moves to the target and
						this row is deleted.
					</Typography>
					<TextField
						label="Target device ID"
						placeholder="UUID of the existing device to merge into"
						value={targetId}
						onChange={(e) => setTargetId(e.target.value.trim())}
						autoFocus
						fullWidth
					/>
					{mergeAction.error && (
						<Alert severity="error">{mergeAction.error.message}</Alert>
					)}
				</Stack>
			</DialogContent>
			<DialogActions>
				<Button onClick={onClose} disabled={mergeAction.pending}>
					Cancel
				</Button>
				<Button
					variant="contained"
					color="warning"
					onClick={onConfirm}
					disabled={mergeAction.pending || targetId === ""}
				>
					{mergeAction.pending ? "Merging…" : "Merge"}
				</Button>
			</DialogActions>
		</Dialog>
	);
}

function DeviceInfoBox({ device }: { device: DeviceInfo }) {
	const conn = device.latest_connection;
	const items: Array<{ label: string; value: React.ReactNode; mono?: boolean }> = [];
	if (conn) items.push({ label: "Address", value: conn.ip, mono: true });
	items.push({
		label: "First seen",
		value: <TimeAgo timestamp={device.device.created_at} />,
	});
	if (conn)
		items.push({
			label: "Last seen",
			value: <TimeAgo timestamp={conn.created_at} />,
		});
	items.push({
		label: "Last updated",
		value: <TimeAgo timestamp={device.device.updated_at} />,
	});
	if (conn?.user_agent)
		items.push({ label: "User-agent", value: conn.user_agent });

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={4}
				useFlexGap
				sx={{ flexWrap: "wrap" }}
			>
				{items.map(({ label, value, mono }) => (
					<Stack key={label} spacing={0.25}>
						<Typography variant="caption" color="text.secondary">
							{label}
						</Typography>
						<Typography
							variant="body2"
							component="div"
							sx={mono ? { fontFamily: "monospace" } : undefined}
						>
							{value}
						</Typography>
					</Stack>
				))}
			</Stack>
		</Paper>
	);
}

function KeysBox({
	device,
	refresh,
}: {
	device: DeviceInfo;
	refresh: () => void;
}) {
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="h6" component="h2" gutterBottom>
				Public Keys ({device.keys.length})
			</Typography>
			<Stack spacing={2}>
				{device.keys.map((k) => (
					<KeyRow key={k.id} keyData={k} onSaved={refresh} />
				))}
			</Stack>
		</Paper>
	);
}

function KeyRow({
	keyData,
	onSaved,
}: {
	keyData: DeviceKeyInfo;
	onSaved: () => void;
}) {
	const [editing, setEditing] = useState(false);
	const [name, setName] = useState(keyData.name ?? "");
	const action = useApiAction("devices", "update_key_name");

	const save = async () => {
		const trimmed = name.trim();
		try {
			await action.call({
				key_id: keyData.id,
				name: trimmed === "" ? null : trimmed,
			});
			setEditing(false);
			onSaved();
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<Box>
			{editing ? (
				<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
					<TextField
						size="small"
						fullWidth
						value={name}
						onChange={(e) => setName(e.target.value)}
						placeholder="Key name (optional)"
						disabled={action.pending}
					/>
					<Button
						variant="contained"
						onClick={save}
						disabled={action.pending}
					>
						{action.pending ? "Saving…" : "Save"}
					</Button>
					<Button
						variant="outlined"
						color="error"
						onClick={() => {
							setName(keyData.name ?? "");
							setEditing(false);
						}}
						disabled={action.pending}
					>
						Cancel
					</Button>
				</Stack>
			) : (
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center", justifyContent: "space-between" }}
				>
					<Typography variant="subtitle1">
						{keyData.name ?? "Unnamed key"}
					</Typography>
					<IconButton
						aria-label={`edit name for ${keyData.name ?? "key"}`}
						size="small"
						onClick={() => setEditing(true)}
					>
						<EditIcon fontSize="small" />
					</IconButton>
				</Stack>
			)}
			{action.error && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{action.error.message}
				</Alert>
			)}
			<Box
				component="pre"
				sx={{
					mt: 1,
					p: 1.5,
					borderRadius: 1,
					bgcolor: "action.hover",
					fontFamily: "monospace",
					fontSize: "0.75em",
					overflow: "auto",
				}}
			>
				{keyData.pem_data}
			</Box>
		</Box>
	);
}

function RoleControls({
	device,
	refresh,
}: {
	device: DeviceInfo;
	refresh: () => void;
}) {
	const role = device.device.role;
	const trustAction = useApiAction("devices", "trust");
	const untrustAction = useApiAction("devices", "untrust");
	const updateRoleAction = useApiAction("devices", "update_role");

	const [selected, setSelected] = useState<DeviceRole>(
		role === "untrusted" ? "server" : role,
	);
	const [confirmUntrust, setConfirmUntrust] = useState(false);

	const onSave = async () => {
		try {
			if (role === "untrusted") {
				await trustAction.call({ device_id: device.device.id, role: selected });
			} else {
				await updateRoleAction.call({
					device_id: device.device.id,
					role: selected,
				});
			}
			refresh();
		} catch {
			/* surfaced via *.error */
		}
	};

	const onUntrust = async () => {
		try {
			await untrustAction.call({ device_id: device.device.id });
			setConfirmUntrust(false);
			refresh();
		} catch {
			/* surfaced via untrustAction.error */
		}
	};

	const pending = trustAction.pending || untrustAction.pending || updateRoleAction.pending;

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
				useFlexGap
			>
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center" }}
				>
					<Typography variant="body2">
						{role === "untrusted" ? "Trust this device as:" : "Change role:"}
					</Typography>
					<TextField
						select
						size="small"
						value={selected}
						onChange={(e) => setSelected(e.target.value as DeviceRole)}
						disabled={pending}
					>
						{TRUSTABLE_ROLES.map((r) => (
							<MenuItem key={r} value={r}>
								{r}
							</MenuItem>
						))}
					</TextField>
					<Button
						variant="contained"
						onClick={onSave}
						disabled={pending}
					>
						{role === "untrusted"
							? trustAction.pending
								? "Trusting…"
								: "Trust"
							: updateRoleAction.pending
								? "Saving…"
								: "Save"}
					</Button>
				</Stack>
				{role !== "untrusted" && (
					<Box>
						{confirmUntrust ? (
							<Stack direction="row" spacing={1}>
								<Button
									variant="contained"
									color="error"
									onClick={onUntrust}
									disabled={pending}
								>
									{untrustAction.pending ? "Untrusting…" : "Confirm untrust"}
								</Button>
								<Button
									variant="outlined"
									onClick={() => setConfirmUntrust(false)}
									disabled={pending}
								>
									Cancel
								</Button>
							</Stack>
						) : (
							<Button
								variant="outlined"
								color="error"
								onClick={() => setConfirmUntrust(true)}
							>
								Untrust
							</Button>
						)}
					</Box>
				)}
			</Stack>
			{(trustAction.error ||
				untrustAction.error ||
				updateRoleAction.error) && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{(trustAction.error ?? untrustAction.error ?? updateRoleAction.error)?.message}
				</Alert>
			)}
		</Paper>
	);
}

function AssociatedServersSection({ deviceId }: { deviceId: string }) {
	const result = useApi(
		"devices",
		"get_servers_for_device",
		{ device_id: deviceId },
		[deviceId],
	);
	return (
		<ServersListSection
			title="Associated servers"
			result={result}
			emptyText="No servers are associated with this device."
		/>
	);
}

function PastServersSection({ deviceId }: { deviceId: string }) {
	const result = useApi(
		"devices",
		"get_past_server_associations",
		{ device_id: deviceId },
		[deviceId],
	);
	return (
		<ServersListSection
			title="Past server associations"
			result={result}
			emptyText="No past server associations found."
		/>
	);
}

function ServersListSection({
	title,
	result,
	emptyText,
}: {
	title: string;
	result: ApiState<ServerShortyInfo[]> & { reload: () => void };
	emptyText: string;
}) {
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="h6" component="h3">
					{title}
				</Typography>
				<IconButton
					aria-label="Refresh"
					size="small"
					onClick={result.reload}
				>
					<RefreshIcon fontSize="small" />
				</IconButton>
			</Stack>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : result.data.length === 0 ? (
				<Alert severity="info">{emptyText}</Alert>
			) : (
				<Stack spacing={1}>
					{result.data.map((s) => (
						<ServerShorty key={s.id} server={s} />
					))}
				</Stack>
			)}
		</Paper>
	);
}

const HISTORY_BATCH = 1000;

interface ConnectionGroup {
	ip: string;
	user_agent: string | null;
	count: number;
	earliest: string;
	latest: string;
}

function ConnectionHistory({ deviceId }: { deviceId: string }) {
	const [show, setShow] = useState(false);
	const count = useApi(
		"devices",
		"connection_count",
		{ device_id: deviceId },
		[deviceId],
	);
	const countLabel = count.status === "ok" ? ` (${count.data})` : "";

	return (
		<Box>
			<Button variant="outlined" onClick={() => setShow((s) => !s)}>
				{show ? "Hide connection history" : "Show connection history"}
				{countLabel}
			</Button>
			{show && <ConnectionHistoryDetail deviceId={deviceId} />}
		</Box>
	);
}

function ConnectionHistoryDetail({ deviceId }: { deviceId: string }) {
	const [connections, setConnections] = useState<DeviceConnectionData[]>([]);
	const [hasMore, setHasMore] = useState(false);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const loadBatch = async (
		before: { created_at: string; id: string } | null,
	) => {
		setLoading(true);
		setError(null);
		try {
			const batch = await callApi(
				"devices",
				"connection_history",
				{ device_id: deviceId, limit: HISTORY_BATCH, before },
			);
			setHasMore(batch.length === HISTORY_BATCH);
			setConnections((prev) => {
				const map = new Map(prev.map((c) => [c.id, c]));
				for (const c of batch) map.set(c.id, c);
				return Array.from(map.values());
			});
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
			setHasMore(false);
		} finally {
			setLoading(false);
		}
	};

	useEffect(() => {
		void loadBatch(null);
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [deviceId]);

	const sorted = [...connections].sort((a, b) =>
		b.created_at.localeCompare(a.created_at),
	);
	const groups = groupConnections(sorted);

	return (
		<Box sx={{ mt: 2 }}>
			{loading && connections.length === 0 && <LinearProgress />}
			{error && <Alert severity="error">{error}</Alert>}
			{!loading && !error && connections.length === 0 && (
				<Alert severity="info">No connection history found.</Alert>
			)}
			{groups.length > 0 && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Stack spacing={1}>
						{groups.map((g, i) => (
							<ConnectionGroupRow key={i} group={g} />
						))}
					</Stack>
				</Paper>
			)}
			{hasMore && (
				<Box sx={{ mt: 1 }}>
					<Button
						variant="outlined"
						onClick={() => {
							const earliest = sorted[sorted.length - 1];
							if (earliest)
								void loadBatch({
									created_at: earliest.created_at,
									id: earliest.id,
								});
						}}
						disabled={loading}
					>
						{loading ? "Loading…" : `Load more (${HISTORY_BATCH})`}
					</Button>
				</Box>
			)}
		</Box>
	);
}

function ConnectionGroupRow({ group }: { group: ConnectionGroup }) {
	const span = humanDuration(group.earliest, group.latest);
	return (
		<Stack
			direction="row"
			spacing={2}
			sx={{ alignItems: "center", justifyContent: "space-between" }}
			useFlexGap
		>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<Typography variant="body2" component="div">
					<TimeAgo timestamp={group.earliest} /> to{" "}
					<TimeAgo timestamp={group.latest} />
				</Typography>
				<Typography variant="body2" color="text.secondary">
					{span}
				</Typography>
				<Typography variant="body2" sx={{ fontFamily: "monospace" }}>
					{group.ip}
				</Typography>
				{group.count > 1 && (
					<Typography variant="body2" color="text.secondary">
						{group.count}×
					</Typography>
				)}
			</Stack>
			{group.user_agent && (
				<Typography variant="body2" color="text.secondary">
					{group.user_agent}
				</Typography>
			)}
		</Stack>
	);
}

function groupConnections(
	connections: DeviceConnectionData[],
): ConnectionGroup[] {
	if (connections.length === 0) return [];
	const groups: ConnectionGroup[] = [];
	let current: DeviceConnectionData[] = [connections[0]];
	for (const c of connections.slice(1)) {
		const last = current[current.length - 1];
		if (c.ip === last.ip && c.user_agent === last.user_agent) {
			current.push(c);
		} else {
			groups.push(toGroup(current));
			current = [c];
		}
	}
	groups.push(toGroup(current));
	return groups;
}

function toGroup(conns: DeviceConnectionData[]): ConnectionGroup {
	const first = conns[0];
	const last = conns[conns.length - 1];
	return {
		ip: first.ip,
		user_agent: first.user_agent,
		count: conns.length,
		earliest: last.created_at,
		latest: first.created_at,
	};
}

