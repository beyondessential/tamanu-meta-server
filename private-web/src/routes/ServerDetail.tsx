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
	Link as MuiLink,
	Paper,
	Stack,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import CancelIcon from "@mui/icons-material/Cancel";
import EditIcon from "@mui/icons-material/Edit";
import LanguageIcon from "@mui/icons-material/Language";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { Fragment, useEffect, useState } from "react";
import { Link as RouterLink, useParams } from "react-router-dom";
import IncidentsLink from "../components/IncidentsLink";
import ManualEventButton from "../components/ManualEventButton";
import StatusDot from "../components/StatusDot";
import TailnetIdentitySection from "../components/TailnetIdentitySection";
import TimeAgo from "../components/TimeAgo";
import TimezoneTooltip from "../components/TimezoneTooltip";
import VersionIndicator from "../components/VersionIndicator";
import { HealthLegend, StatusLegend, VersionLegend } from "../components/Legends";
import ServerKindChip from "../components/ServerKindChip";
import ServerRankChip from "../components/ServerRankChip";
import { callApi, useApi, useApiAction } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { humanSeconds } from "../lib/humanDuration";
import ServerNameWithGroup from "../components/ServerNameWithGroup";
import type {
	DeviceInfo,
	ServerDetailData,
	ServerGroup,
	ServerInfo,
	ServerLastStatusData,
} from "../types";

export default function ServerDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const detail = useApi(
		"servers",
		"get_detail",
		{ server_id: id },
		[id],
	);
	const isAdmin = useApi("commons", "is_current_user_admin");
	// Single refresh signal for everything on the page that talks to the
	// issues/incidents APIs. Any mutation (manual-event submit, resolve/
	// snooze on a row, etc.) bumps this so all sibling panels refetch in
	// lockstep — otherwise resolving an issue (which can auto-close an
	// incident) leaves a stale incidents list.
	const [refreshTick, setRefreshTick] = useState(0);
	const bumpRefresh = () => setRefreshTick((t) => t + 1);
	// Single source of truth for the group's open-incident state. Used to
	// label every ManualEventButton on the page identically — a child
	// server's own incidents list is empty (incidents live at the root),
	// so per-button local queries would mislabel.
	const openIncidents = useApi(
		"incidents",
		"list_for_server",
		{ server_id: id, include_closed: false },
		[id, refreshTick],
	);
	const hasOpenIncident =
		openIncidents.status === "ok" && openIncidents.data.length > 0;
	usePageTitle(
		detail.status === "ok"
			? (detail.data.server.name ?? "Unnamed server")
			: "Server",
	);

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <Alert severity="error">{detail.error.message}</Alert>;
	}

	const data = detail.data;
	const admin = isAdmin.status === "ok" && isAdmin.data;

	return (
		<Stack spacing={3}>
			<Header
				data={data}
				isAdmin={admin}
				hasOpenIncident={hasOpenIncident}
				refreshTick={refreshTick}
				onEventSubmitted={bumpRefresh}
			/>
			<UrlAndDevice
				host={data.server.host}
				serverId={data.server.id}
				deviceInfo={data.device_info}
				refresh={() => detail.reload()}
			/>
			{data.device_info && (
				<TailnetIdentitySection
					device={data.device_info}
					refresh={() => detail.reload()}
				/>
			)}
			<InfoSection
				server={data.server}
				status={data.last_status}
			/>
			{data.group && <GroupSection group={data.group} />}
			{(data.server.notes || Object.keys(data.server.tags ?? {}).length > 0) && (
				<NotesAndTagsSection
					notes={data.server.notes}
					tags={data.server.tags}
				/>
			)}
			{data.siblings.length > 0 && (
				<SiblingServers
					siblings={data.siblings}
					isAdmin={admin}
					hasOpenIncident={hasOpenIncident}
					onEventSubmitted={bumpRefresh}
				/>
			)}
			<Box>
				<VersionLegend />
				<Box sx={{ mt: 1 }}>
					<StatusLegend />
				</Box>
				<Box sx={{ mt: 1 }}>
					<HealthLegend />
				</Box>
			</Box>
		</Stack>
	);
}

function Header({
	data,
	isAdmin,
	hasOpenIncident,
	refreshTick,
	onEventSubmitted,
}: {
	data: ServerDetailData;
	isAdmin: boolean;
	hasOpenIncident: boolean;
	refreshTick: number;
	onEventSubmitted: () => void;
}) {
	return (
		<Stack
			direction="row"
			spacing={2}
			sx={{ alignItems: "center", justifyContent: "space-between" }}
		>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				{data.server.rank && <ServerRankChip rank={data.server.rank} />}
				<ServerKindChip kind={data.server.kind} />
				<Typography variant="h4" component="h1" sx={{ ml: 1 }}>
					<StatusDot
						up={data.up}
						health={data.health}
						title={data.server.name ?? ""}
						size="0.8em"
					/>
					{data.siblings.map(([up, health, sib]) => (
						<StatusDot
							key={sib.id}
							up={up}
							health={health}
							title={sib.name ?? ""}
							dim
							size="0.8em"
						/>
					))}
					<ServerNameWithGroup
						groupName={data.server.group_name}
						serverName={data.server.name ?? "Unnamed"}
					/>
				</Typography>
			</Stack>
			<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
				<IncidentsLink
					serverId={data.server.id}
					refreshKey={refreshTick}
				/>
				{isAdmin && (
					<>
						<ManualEventButton
							serverId={data.server.id}
							hasOpenIncident={hasOpenIncident}
							onSubmitted={onEventSubmitted}
						/>
						<Button
							component={RouterLink}
							to={`/servers/${data.server.id}/edit`}
							variant="contained"
							startIcon={<EditIcon />}
						>
							Edit
						</Button>
					</>
				)}
			</Stack>
		</Stack>
	);
}

function UrlAndDevice({
	host,
	serverId,
	deviceInfo,
	refresh,
}: {
	host: string;
	serverId: string;
	deviceInfo: DeviceInfo | null;
	refresh: () => void;
}) {
	const [attachOpen, setAttachOpen] = useState(false);
	return (
		<Stack
			direction={{ xs: "column", md: "row" }}
			spacing={2}
			useFlexGap
			sx={{ "& > *": { flex: 1 } }}
		>
			<Paper variant="outlined" sx={{ p: 2 }}>
				<Typography variant="h6" component="h2" gutterBottom>
					URL
				</Typography>
				<MuiLink href={host} target="_blank" rel="noopener noreferrer">
					{host}
				</MuiLink>
			</Paper>
			<Paper variant="outlined" sx={{ p: 2 }}>
				<Typography variant="h6" component="h2" gutterBottom>
					Device
				</Typography>
				{deviceInfo ? (
					<Stack
						direction="row"
						spacing={1}
						sx={{ alignItems: "center", flexWrap: "wrap" }}
						useFlexGap
					>
						<MuiLink
							component={RouterLink}
							to={`/devices/${deviceInfo.device.id}`}
							underline="hover"
						>
							{deviceShortName(deviceInfo)}
						</MuiLink>
						{deviceInfo.device.tailscale_node_id != null && (
							<Chip
								size="small"
								variant="outlined"
								color="success"
								label="tailnet"
							/>
						)}
						{deviceInfo.keys.length > 0 && (
							<Chip
								size="small"
								variant="outlined"
								label="mTLS"
							/>
						)}
					</Stack>
				) : (
					<Stack spacing={1} sx={{ alignItems: "flex-start" }}>
						<Typography variant="body2" color="text.secondary">
							No device attached. Bind this server to a Tailscale
							node and canopy will auto-create the device row if
							it doesn't exist yet.
						</Typography>
						<Button
							variant="contained"
							onClick={() => setAttachOpen(true)}
						>
							Attach Tailscale device
						</Button>
					</Stack>
				)}
			</Paper>
			<AttachServerDeviceDialog
				open={attachOpen}
				onClose={() => setAttachOpen(false)}
				serverId={serverId}
				onAttached={() => {
					setAttachOpen(false);
					refresh();
				}}
			/>
		</Stack>
	);
}

function AttachServerDeviceDialog({
	open,
	onClose,
	serverId,
	onAttached,
}: {
	open: boolean;
	onClose: () => void;
	serverId: string;
	onAttached: () => void;
}) {
	const [identifier, setIdentifier] = useState("");
	const [preview, setPreview] = useState<
		import("../types").TailnetLiveInfo | null
	>(null);
	const [previewError, setPreviewError] = useState<string | null>(null);
	const [previewLoading, setPreviewLoading] = useState(false);
	const attachAction = useApiAction("servers", "attach_tailscale_device");

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
			await attachAction.call({ server_id: serverId, identifier });
			onAttached();
		} catch {
			/* surfaced via attachAction.error */
		}
	};

	return (
		<Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
			<DialogTitle>Attach Tailscale device to server</DialogTitle>
			<DialogContent>
				<Stack spacing={2} sx={{ mt: 1 }}>
					<Typography variant="body2" color="text.secondary">
						Paste any Tailscale identifier — IP, node ID, or DNS
						name. If a device row exists for that node, it's
						attached to the server; otherwise canopy creates a new
						device row first.
					</Typography>
					<TextField
						label="Tailscale identifier"
						placeholder="100.64.0.42 / nodekey:… / device.example.ts.net"
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
							</Stack>
						</Paper>
					)}
					{previewError && identifier.trim() !== "" && (
						<Alert severity="info">{previewError}</Alert>
					)}
					{attachAction.error && (
						<Alert severity="error">{attachAction.error.message}</Alert>
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

function deviceShortName(info: DeviceInfo): string {
	const namedKey = info.keys.findLast(
		(k) => k.name && k.name !== "Initial Key",
	);
	if (namedKey?.name) return namedKey.name;
	if (info.latest_connection) return info.latest_connection.ip;
	return info.device.id;
}

function InfoSection({
	server,
	status,
}: {
	server: ServerInfo;
	status: ServerLastStatusData | null;
}) {
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			{status && <HealthIndicator healthy={status.healthy} />}
			<Stack
				direction="row"
				spacing={4}
				useFlexGap
				sx={{ flexWrap: "wrap" }}
			>
				{status && <StatusInfoFields status={status} />}
				{server.kind === "central" && (
					<InfoItem label="Mobile list" value={server.listed ? "Public" : "No"} />
				)}
				<InfoItem
					label="Status alerts"
					value={
						server.alert_when_down_for > 0
							? `After ${humanSeconds(server.alert_when_down_for)}`
							: "Off"
					}
				/>
				{server.cloud != null && (
					<InfoItem
						label="Location"
						value={renderLocation(server)}
					/>
				)}
			</Stack>
			{status && (
				<ChecksTable health={status.health} overallHealthy={status.healthy} />
			)}
			{status && Object.keys((status.extra ?? {}) as Record<string, unknown>).length > 0 && (
				<Box sx={{ mt: 2 }}>
					<details>
						<summary>Extra Data</summary>
						<Box
							component="pre"
							sx={{
								mt: 1,
								p: 1.5,
								borderRadius: 1,
								bgcolor: "action.hover",
								overflow: "auto",
								fontSize: "0.85em",
							}}
						>
							{JSON.stringify(status.extra, null, 2)}
						</Box>
					</details>
				</Box>
			)}
		</Paper>
	);
}

/** Global health chip rendered at the top of the InfoSection. The
 * server's per-check breakdown is shown by `<ChecksTable>` below — this
 * is the "headline" answer to "does the server think it's OK". */
function HealthIndicator({ healthy }: { healthy: boolean }) {
	return (
		<Box sx={{ mb: 1.5 }}>
			<Chip
				size="small"
				color={healthy ? "success" : "error"}
				icon={healthy ? <CheckCircleIcon /> : <CancelIcon />}
				label={healthy ? "Healthy" : "Unhealthy"}
			/>
		</Box>
	);
}

/** Per-check table from the most recent status push. Failing entries
 * sort first so the operator sees them without scrolling, then
 * alphabetical. Capped at 5 visible rows with an "expand all" toggle
 * so a server reporting 30 checks doesn't push the rest of the page
 * off-screen. Render nothing when the server doesn't ship per-check
 * data (legacy / minimal payloads). */
function ChecksTable({
	health,
	overallHealthy,
}: {
	health: ServerLastStatusData["health"];
	overallHealthy: boolean;
}) {
	const entries = parseChecks(health);
	const [expanded, setExpanded] = useState(false);
	if (entries.length === 0) return null;
	const HIDE_AFTER = 5;
	const visible = expanded ? entries : entries.slice(0, HIDE_AFTER);
	const hidden = entries.length - visible.length;
	return (
		<Box sx={{ mt: 2 }}>
			<Typography variant="overline" color="text.secondary">
				Checks ({entries.length})
			</Typography>
			<Stack spacing={1} sx={{ mt: 0.5 }}>
				{visible.map((entry) => (
					<CheckRow
						key={entry.check}
						entry={entry}
						overallHealthy={overallHealthy}
					/>
				))}
			</Stack>
			{hidden > 0 && (
				<Button
					size="small"
					onClick={() => setExpanded(true)}
					sx={{ mt: 0.5 }}
				>
					Show {hidden} more
				</Button>
			)}
			{expanded && entries.length > HIDE_AFTER && (
				<Button
					size="small"
					onClick={() => setExpanded(false)}
					sx={{ mt: 0.5 }}
				>
					Collapse
				</Button>
			)}
		</Box>
	);
}

type ParsedCheck = {
	check: string;
	healthy: boolean;
	/** Everything other than `check` and `healthy`, preserved in source
	 * order. */
	extras: Array<[string, unknown]>;
};

function parseChecks(health: ServerLastStatusData["health"]): ParsedCheck[] {
	if (!Array.isArray(health)) return [];
	const parsed: ParsedCheck[] = [];
	for (const raw of health as unknown[]) {
		if (typeof raw !== "object" || raw === null) continue;
		const obj = raw as Record<string, unknown>;
		const check = obj.check;
		const healthy = obj.healthy;
		if (typeof check !== "string" || typeof healthy !== "boolean") continue;
		const extras: Array<[string, unknown]> = Object.entries(obj).filter(
			([k]) => k !== "check" && k !== "healthy",
		);
		parsed.push({ check, healthy, extras });
	}
	// Failing first, then alphabetical by name. Stable: same input
	// always produces the same visible order.
	parsed.sort((a, b) => {
		if (a.healthy !== b.healthy) return a.healthy ? 1 : -1;
		return a.check.localeCompare(b.check);
	});
	return parsed;
}

function CheckRow({
	entry,
	overallHealthy,
}: {
	entry: ParsedCheck;
	overallHealthy: boolean;
}) {
	return (
		<Stack
			direction="row"
			spacing={1.5}
			sx={{
				p: 1,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
				alignItems: "flex-start",
				bgcolor: entry.healthy ? undefined : "action.hover",
			}}
		>
			{entry.healthy ? (
				<CheckCircleIcon fontSize="small" color="success" />
			) : overallHealthy ? (
				<WarningAmberIcon fontSize="small" color="warning" />
			) : (
				<CancelIcon fontSize="small" color="error" />
			)}
			<Box sx={{ flex: 1, minWidth: 0 }}>
				<Typography variant="body2" sx={{ fontFamily: "monospace" }}>
					{entry.check}
				</Typography>
				{entry.extras.length > 0 && (
					<Box
						component="dl"
						sx={{
							m: 0,
							mt: 0.5,
							display: "grid",
							gridTemplateColumns: "max-content 1fr",
							columnGap: 1.5,
							rowGap: 0.25,
							fontSize: "0.8em",
						}}
					>
						{entry.extras.map(([k, v]) => (
							<Fragment key={k}>
								<Box component="dt" sx={{ color: "text.secondary" }}>
									{k}
								</Box>
								<Box
									component="dd"
									sx={{ m: 0, fontFamily: "monospace" }}
								>
									{renderCheckValue(v)}
								</Box>
							</Fragment>
						))}
					</Box>
				)}
			</Box>
		</Stack>
	);
}

function renderCheckValue(v: unknown): string {
	if (typeof v === "string") return v;
	if (v === null) return "null";
	return JSON.stringify(v);
}

function StatusInfoFields({ status }: { status: ServerLastStatusData }) {
	return (
		<>
			<Stack spacing={0.25}>
				<Typography variant="caption" color="text.secondary">
					Last seen
				</Typography>
				<Typography variant="body2" component="div">
					<TimeAgo timestamp={status.created_at} />
				</Typography>
			</Stack>
			{status.platform && (
				<InfoItem label="Platform" value={status.platform} />
			)}
			{status.timezone && (
				<InfoItem label="Timezone">
					<Typography variant="body2">
						<TimezoneTooltip tz={status.timezone} />
					</Typography>
				</InfoItem>
			)}
			<Stack spacing={0.25}>
				<Typography variant="caption" color="text.secondary">
					Tamanu
				</Typography>
				<VersionIndicator
					version={status.version}
					distance={status.version_distance}
				/>
			</Stack>
			{status.postgres && (
				<InfoItem
					label="PostgreSQL"
					value={status.postgres}
					mono
				/>
			)}
			{status.nodejs && (
				<InfoItem label="Node.js" value={status.nodejs} mono />
			)}
			{status.min_chrome_version != null && (
				<InfoItem
					label="Chrome"
					value={`${status.min_chrome_version} or later`}
					mono
				/>
			)}
		</>
	);
}

function InfoItem({
	label,
	value,
	mono = false,
	children,
}: {
	label: string;
	value?: string;
	mono?: boolean;
	children?: React.ReactNode;
}) {
	return (
		<Stack spacing={0.25}>
			<Typography variant="caption" color="text.secondary">
				{label}
			</Typography>
			{children ?? (
				<Typography
					variant="body2"
					sx={mono ? { fontFamily: "monospace" } : undefined}
				>
					{value}
				</Typography>
			)}
		</Stack>
	);
}

function renderLocation(server: ServerInfo): string {
	if (!server.cloud) return "On premise";
	return "Cloud";
}

function SiblingServers({
	siblings,
	isAdmin,
	hasOpenIncident,
	onEventSubmitted,
}: {
	siblings: ServerDetailData["siblings"];
	isAdmin: boolean;
	hasOpenIncident: boolean;
	onEventSubmitted: () => void;
}) {
	return (
		<Box>
			<Typography variant="h5" component="h2" gutterBottom>
				Other servers in this group ({siblings.length})
			</Typography>
			<Stack spacing={1}>
				{siblings.map(([up, health, sib]) => (
					<Stack
						key={sib.id}
						direction="row"
						spacing={1}
						sx={{
							p: 1.5,
							border: 1,
							borderColor: "divider",
							borderRadius: 1,
							alignItems: "center",
						}}
					>
						<Tooltip title={sib.host}>
							<IconButton
								component="a"
								href={sib.host}
								target="_blank"
								rel="noopener noreferrer"
								size="small"
								aria-label={`Open ${sib.name ?? "server"} (${sib.host})`}
							>
								<LanguageIcon fontSize="small" />
							</IconButton>
						</Tooltip>
						<StatusDot up={up} health={health} />
						<MuiLink
							component={RouterLink}
							to={`/servers/${sib.id}`}
							underline="hover"
							color="text.primary"
							sx={{ fontWeight: 500 }}
						>
							{sib.name ?? "Unnamed"}
						</MuiLink>
						{sib.rank && <ServerRankChip rank={sib.rank} />}
						<ServerKindChip kind={sib.kind} />
						{sib.alert_when_down_for <= 0 && (
							<Tooltip title="Status alerts are off for this server — canopy isn't watching it.">
								<Chip
									size="small"
									variant="outlined"
									label="unmonitored"
								/>
							</Tooltip>
						)}
						<Box sx={{ flex: 1 }} />
						{isAdmin && (
							<ManualEventButton
								serverId={sib.id}
								hasOpenIncident={hasOpenIncident}
								onSubmitted={onEventSubmitted}
							/>
						)}
					</Stack>
				))}
			</Stack>
		</Box>
	);
}

function GroupSection({ group }: { group: ServerGroup }) {
	const tagEntries = Object.entries(group.tags ?? {});
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="h6" component="h2">
					Group
				</Typography>
				<MuiLink
					component={RouterLink}
					to={`/groups/${group.id}`}
					underline="hover"
				>
					{group.name}
				</MuiLink>
			</Stack>
			{group.notes && (
				<Typography
					variant="body2"
					sx={{ whiteSpace: "pre-wrap", color: "text.secondary", mb: 1 }}
				>
					{group.notes}
				</Typography>
			)}
			{tagEntries.length > 0 && (
				<Stack direction="row" sx={{ flexWrap: "wrap", gap: 0.5 }}>
					{tagEntries.map(([k, v]) => (
						<Chip
							key={k}
							size="small"
							variant="outlined"
							label={`${k}=${v}`}
						/>
					))}
				</Stack>
			)}
		</Paper>
	);
}

function NotesAndTagsSection({
	notes,
	tags,
}: {
	notes: string;
	tags: Record<string, string>;
}) {
	const tagEntries = Object.entries(tags ?? {});
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="h6" component="h2" gutterBottom>
				Notes & tags
			</Typography>
			{notes && (
				<Typography variant="body2" sx={{ whiteSpace: "pre-wrap", mb: 1 }}>
					{notes}
				</Typography>
			)}
			{tagEntries.length > 0 && (
				<Stack direction="row" sx={{ flexWrap: "wrap", gap: 0.5 }}>
					{tagEntries.map(([k, v]) => (
						<Chip
							key={k}
							size="small"
							variant="outlined"
							label={`${k}=${v}`}
						/>
					))}
				</Stack>
			)}
		</Paper>
	);
}
