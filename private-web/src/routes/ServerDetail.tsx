import {
	Accordion,
	AccordionDetails,
	AccordionSummary,
	Alert,
	Box,
	Button,
	Chip,
	Collapse,
	Dialog,
	DialogActions,
	DialogContent,
	DialogContentText,
	DialogTitle,
	Divider,
	IconButton,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Popover,
	Stack,
	Switch,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import BuildCircleIcon from "@mui/icons-material/BuildCircle";
import BuildOutlinedIcon from "@mui/icons-material/BuildOutlined";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import CancelIcon from "@mui/icons-material/Cancel";
import RemoveCircleOutlinedIcon from "@mui/icons-material/RemoveCircleOutlined";
import ArchiveIcon from "@mui/icons-material/ArchiveOutlined";
import EditIcon from "@mui/icons-material/Edit";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import InsightsIcon from "@mui/icons-material/Insights";
import LanguageIcon from "@mui/icons-material/Language";
import RestoreIcon from "@mui/icons-material/RestoreFromTrash";
import RestoreDataIcon from "@mui/icons-material/SettingsBackupRestore";
import NotificationsActiveOutlinedIcon from "@mui/icons-material/NotificationsActiveOutlined";
import NotificationsOffIcon from "@mui/icons-material/NotificationsOff";
import NotificationsOffOutlinedIcon from "@mui/icons-material/NotificationsOffOutlined";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { useEffect, useState } from "react";
import {
	Link as RouterLink,
	useLocation,
	useNavigate,
	useParams,
} from "react-router-dom";
import ActionButton from "../components/ActionButton";
import CheckDocButton from "../components/CheckDocButton";
import CheckExtrasList, { checkEntryExtras } from "../components/CheckExtras";
import ExternalUsersDetails, {
	parseExternalUserSessions,
} from "../components/ExternalUsersDetails";
import HealthChip from "../components/HealthChip";
import IncidentsLink from "../components/IncidentsLink";
import OperatorAvatars from "../components/OperatorAvatars";
import ManualEventButton from "../components/ManualEventButton";
import ServerCertificatesSection from "../components/ServerCertificatesSection";
import MaintenanceSection from "../components/MaintenanceSection";
import SilencedRefsSection from "../components/SilencedRefsSection";
import StatusDot from "../components/StatusDot";
import TailnetIdentitySection from "../components/TailnetIdentitySection";
import TimeAgo from "../components/TimeAgo";
import { LatestSnapshot } from "../components/SnapshotId";
import { BackupProcessingChip } from "../components/BackupProcessingChip";
import { BackupLiveProgress } from "../components/BackupLiveProgress";
import TimezoneTooltip from "../components/TimezoneTooltip";
import VersionIndicator from "../components/VersionIndicator";
import ServerProductChip from "../components/ServerProductChip";
import { useProductCaps } from "../hooks/useProducts";
import { PRODUCT_LABELS } from "../types";
import { HealthLegend, StatusLegend, VersionLegend } from "../components/Legends";
import ServerKindChip from "../components/ServerKindChip";
import ServerRankChip from "../components/ServerRankChip";
import ServerSetupInstructions from "../components/ServerSetupInstructions";
import { callApi, useApi, useApiAction } from "../api";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { humanSeconds } from "../lib/humanDuration";
import ServerNameWithGroup from "../components/ServerNameWithGroup";
import {
	compareServersByRankThenKind,
	groupServersByRank,
	healthcheckPath,
	silenceRef,
	type CheckResult,
	type ConsolidatedCheck,
	type ConsolidatedChecks,
	type DeviceInfo,
	type HealthState,
	type OperatorPresence,
	type ServerBackupCapabilityView,
	type ServerDetailData,
	type ServerGroup,
	type ServerGroupSilencedRef,
	type ServerInfo,
	type ServerLastStatusData,
	type ServerSilencedRef,
	type ShortStatus,
} from "../types";

export default function ServerDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const detail = useApi(
		"servers",
		"get_detail",
		{ server_id: id },
		[id],
	);
	const admin = useIsAdmin() === true;
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
	// Honour a `#backups` anchor (linked from the group's backup page): once the
	// detail has loaded and the section is painted, scroll it into view.
	const location = useLocation();
	const detailLoaded = detail.status === "ok";
	useEffect(() => {
		if (!detailLoaded || location.hash !== "#backups") return;
		const frame = requestAnimationFrame(() => {
			document
				.getElementById("backups")
				?.scrollIntoView({ behavior: "smooth", block: "start" });
		});
		return () => cancelAnimationFrame(frame);
	}, [detailLoaded, location.hash]);
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
	const archived = data.server.archived;
	const registered = data.server.registered_at != null;

	return (
		<Stack spacing={3}>
			<Header
				data={data}
				isAdmin={admin}
				hasOpenIncident={hasOpenIncident}
				refreshTick={refreshTick}
				onEventSubmitted={bumpRefresh}
				onArchived={() => detail.reload()}
			/>
			{archived ? (
				<ArchivedBanner
					serverId={data.server.id}
					isAdmin={admin}
					onRestored={() => detail.reload()}
				/>
			) : (
				!registered && (
					<>
						<Alert severity="info">
							This server hasn't checked in yet. Follow the setup
							instructions below to enroll it.
						</Alert>
						<ServerSetupInstructions
							serverId={data.server.id}
							onRegistered={() => detail.reload()}
						/>
					</>
				)
			)}
			<InfoSection
				server={data.server}
				status={data.last_status}
				health={data.health}
				checks={data.checks}
				onSilenced={bumpRefresh}
				up={data.up}
				maintained={data.maintained}
				maintenanceSettling={data.maintenance_settling}
				refreshTick={refreshTick}
			/>
			{data.group && (
				<GroupSection group={data.group} billingLabels={data.billing_labels} />
			)}
			<BackupCapabilitiesSection
				serverId={data.server.id}
				groupId={data.group?.id ?? null}
				isAdmin={admin}
			/>
			{(data.server.notes || Object.keys(data.server.tags ?? {}).length > 0) && (
				<NotesAndTagsSection
					notes={data.server.notes}
					tags={data.server.tags}
				/>
			)}
			<ServerCertificatesSection serverId={data.server.id} />
			<AdvancedIdentitySection
				host={data.server.display_host}
				serverId={data.server.id}
				deviceInfo={data.device_info}
				isAdmin={admin}
				registered={registered}
				refresh={() => detail.reload()}
			/>
			{data.siblings.length > 0 && (
				<SiblingServers
					siblings={data.siblings}
					isAdmin={admin}
					hasOpenIncident={hasOpenIncident}
					onEventSubmitted={bumpRefresh}
				/>
			)}
			<MaintenanceSection
				scope="server"
				anchor="maintenance"
				id={data.server.id}
				targetLabel={data.server.name ?? data.server.display_host}
				groupId={data.group?.id ?? null}
				groupName={data.group?.name ?? null}
				rank={data.server.rank ?? null}
				onChanged={() => detail.reload()}
			/>
			<SilencedRefsSection
				scope="server"
				id={data.server.id}
				refreshKey={refreshTick}
				onChanged={bumpRefresh}
			/>
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
	onArchived,
}: {
	data: ServerDetailData;
	isAdmin: boolean;
	hasOpenIncident: boolean;
	refreshTick: number;
	onEventSubmitted: () => void;
	onArchived: () => void;
}) {
	const archived = data.server.archived;
	// Munin runs on the server's own box, reachable over the tailnet — build
	// its URL from the bound device's MagicDNS name (live value preferred,
	// falling back to the stored snapshot). Only offered when the server is
	// known to run Munin and has a tailnet name.
	// spec: SVC#munin-link
	const tailnetName =
		data.device_info?.tailnet_live?.display_name ??
		data.device_info?.device?.tailscale_node_name ??
		null;
	const muninUrl =
		data.munin && tailnetName ? `https://${tailnetName}:4950/` : null;
	return (
		<Stack spacing={1.5}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				{data.server.rank && <ServerRankChip rank={data.server.rank} />}
				<ServerProductChip product={data.server.product} />
				<ServerKindChip kind={data.server.kind} />
				<Typography variant="h4" component="h1" sx={{ ml: 1 }}>
					<SiblingDotStrip
						focused={data.server}
						focusedUp={data.up}
						focusedHealth={data.health}
						siblings={data.siblings}
					/>
					<ServerNameWithGroup
						groupName={data.server.group_name}
						groupId={data.server.group_id}
						serverName={data.server.name ?? "Unnamed"}
					/>
				</Typography>
			</Stack>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				{data.server.display_host && (
					<ActionButton
						href={data.server.display_host}
						icon={<LanguageIcon />}
						label="Open"
						title={data.server.display_host}
					/>
				)}
				{muninUrl && (
					<ActionButton href={muninUrl} icon={<InsightsIcon />} label="Munin" />
				)}
				<IncidentsLink
					serverId={data.server.id}
					groupId={data.group?.id ?? null}
					refreshKey={refreshTick}
				/>
				{isAdmin && (
					<>
						<ManualEventButton
							serverId={data.server.id}
							hasOpenIncident={hasOpenIncident}
							onSubmitted={onEventSubmitted}
							action
						/>
						<ActionButton
							to={`/servers/${data.server.id}/edit`}
							icon={<EditIcon />}
							label="Edit"
							color="primary"
						/>
						{!archived && (
							<DeleteServerButton
								serverId={data.server.id}
								serverName={data.server.name ?? "this server"}
								groupId={data.server.group_id ?? null}
								onArchived={onArchived}
							/>
						)}
					</>
				)}
			</Stack>
		</Stack>
	);
}

/// Inline admin action: archive (soft-delete) the server behind a confirm
/// dialog, then navigate back to its group (if it had one) or the server list.
function DeleteServerButton({
	serverId,
	serverName,
	groupId,
	onArchived,
}: {
	serverId: string;
	serverName: string;
	groupId: string | null;
	onArchived: () => void;
}) {
	const navigate = useNavigate();
	const action = useApiAction("servers", "delete");
	const [open, setOpen] = useState(false);

	const onConfirm = async () => {
		try {
			await action.call({ server_id: serverId });
			setOpen(false);
			onArchived();
			navigate(groupId ? `/groups/${groupId}` : "/servers");
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<>
			<ActionButton
				color="error"
				icon={<ArchiveIcon />}
				label="Archive"
				onClick={() => setOpen(true)}
			/>
			<Dialog open={open} onClose={() => setOpen(false)} maxWidth="sm" fullWidth>
				<DialogTitle>Archive server?</DialogTitle>
				<DialogContent>
					<DialogContentText>
						Archive <strong>{serverName}</strong>? This soft-deletes the
						server — it stops being monitored and its device is released,
						but its history is kept and it can be restored later.
					</DialogContentText>
					{action.error && (
						<Alert severity="error" sx={{ mt: 2 }}>
							{action.error.message}
						</Alert>
					)}
				</DialogContent>
				<DialogActions>
					<Button onClick={() => setOpen(false)} disabled={action.pending}>
						Cancel
					</Button>
					<Button
						variant="contained"
						color="error"
						onClick={onConfirm}
						disabled={action.pending}
					>
						{action.pending ? "Archiving…" : "Archive"}
					</Button>
				</DialogActions>
			</Dialog>
		</>
	);
}

/// Shown in place of the setup/registration area when a server has been
/// archived. Keeps the rest of the page (history, status, etc.) visible.
function ArchivedBanner({
	serverId,
	isAdmin,
	onRestored,
}: {
	serverId: string;
	isAdmin: boolean;
	onRestored: () => void;
}) {
	const action = useApiAction("servers", "restore");
	const onRestore = async () => {
		try {
			await action.call({ server_id: serverId });
			onRestored();
		} catch {
			/* surfaced via action.error */
		}
	};
	return (
		<Alert
			severity="warning"
			action={
				isAdmin ? (
					<Button
						color="inherit"
						size="small"
						startIcon={<RestoreIcon />}
						onClick={onRestore}
						disabled={action.pending}
					>
						{action.pending ? "Restoring…" : "Restore"}
					</Button>
				) : undefined
			}
		>
			This server is archived. Its history is preserved below; restore it to
			resume monitoring.
			{action.error && (
				<Box sx={{ mt: 1 }}>{action.error.message}</Box>
			)}
		</Alert>
	);
}

/// Shows the server's URL (when it has one) and folds the device + Tailscale
/// identity detail into a collapsed-by-default "Identity" accordion, since the
/// device is now an internal implementation detail of enrollment rather than
/// something operators set up by hand.
function AdvancedIdentitySection({
	host,
	serverId,
	deviceInfo,
	isAdmin,
	registered,
	refresh,
}: {
	host: string;
	serverId: string;
	deviceInfo: DeviceInfo | null;
	isAdmin: boolean;
	registered: boolean;
	refresh: () => void;
}) {
	return (
		<Stack spacing={2}>
			{host && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Typography variant="h6" component="h2" gutterBottom>
						URL
					</Typography>
					<MuiLink href={host} target="_blank" rel="noopener noreferrer">
						{host}
					</MuiLink>
				</Paper>
			)}
			<Accordion variant="outlined" disableGutters>
				<AccordionSummary expandIcon={<ExpandMoreIcon />}>
					<Typography variant="h6" component="h2">
						Identity
					</Typography>
				</AccordionSummary>
				<AccordionDetails>
					<Stack spacing={2}>
						<DeviceCard
							serverId={serverId}
							deviceInfo={deviceInfo}
							refresh={refresh}
						/>
						{deviceInfo && (
							<TailnetIdentitySection
								device={deviceInfo}
								refresh={refresh}
							/>
						)}
						{isAdmin && registered && (
							<ServerSetupInstructions
								serverId={serverId}
								reEnroll
								onRegistered={refresh}
							/>
						)}
					</Stack>
				</AccordionDetails>
			</Accordion>
		</Stack>
	);
}

function DeviceCard({
	serverId,
	deviceInfo,
	refresh,
}: {
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
	health,
	checks,
	onSilenced,
	up,
	maintained,
	maintenanceSettling,
	refreshTick,
}: {
	server: ServerInfo;
	status: ServerLastStatusData | null;
	health: HealthState;
	checks: ConsolidatedChecks;
	onSilenced: () => void;
	up: ShortStatus;
	maintained: boolean;
	maintenanceSettling: boolean;
	refreshTick: number;
}) {
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			{status && (
				<HealthIndicator
					health={health}
					up={up}
					monitored={server.is_monitored !== false}
					maintained={maintained}
					maintenanceSettling={maintenanceSettling}
					operators={status.operators}
				/>
			)}
			<Stack
				direction="row"
				spacing={4}
				useFlexGap
				sx={{ flexWrap: "wrap" }}
			>
				{status && <StatusInfoFields status={status} />}
				{server.kind === "central" && (
					<InfoItem
						label="Mobile list"
						value={server.public_name ?? "Not listed"}
					/>
				)}
				<InfoItem
					label="Status alerts"
					value={
						server.is_monitored
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
				{/* Only where something has been granted. "Not permitted" on every
				    server in the fleet advertises a feature that a Canopy instance
				    without DNS zones does not have.
				    spec: DOM#permission-for-a-server-to-manage-its-own-names */}
				{(server.may_manage_dns || server.may_manage_tls) && (
					<InfoItem
						label="Name management"
						value={nameManagementLabel(server)}
					/>
				)}
			</Stack>
			<ChecksTable
				checks={checks}
				operators={status?.operators ?? []}
				serverId={server.id}
				groupId={server.group_id}
				maintained={maintained}
				refreshTick={refreshTick}
				onSilenced={onSilenced}
			/>
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
 * is the "headline" answer to "is the server OK", derived from the
 * `HealthState` rollup rather than the raw top-level `healthy` bool so
 * a failing check can't hide behind a self-reported "healthy".
 *
 * Alongside it, the operator-presence headline: identified humans
 * connected to the server per the `external_users` check. Only asserted
 * while the server is actively reporting — a stale push can't claim
 * anyone is in the server *right now*. */
function HealthIndicator({
	health,
	up,
	monitored,
	maintained,
	maintenanceSettling,
	operators,
}: {
	health: HealthState;
	up: ShortStatus;
	monitored: boolean;
	maintained: boolean;
	maintenanceSettling: boolean;
	operators: OperatorPresence[];
}) {
	const reporting = up === "up" || up === "blip";
	return (
		<Stack
			direction="row"
			spacing={2}
			useFlexGap
			sx={{ mb: 1.5, alignItems: "center", flexWrap: "wrap" }}
		>
			<HealthChip
				health={health}
				stale={!reporting}
				monitored={monitored}
				maintained={maintained}
				maintenanceSettling={maintenanceSettling}
				maintenanceHref="#maintenance"
			/>
			{reporting && operators.length > 0 && (
				<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
					<OperatorAvatars operators={operators} size={24} />
					<Typography variant="body2">
						{operators.length} operator
						{operators.length === 1 ? "" : "s"} in the server right now
					</Typography>
				</Stack>
			)}
		</Stack>
	);
}

/** Consolidated per-check table: every source's current checks, graded
 * and sorted most-urgent-first by the backend. Capped at 5 visible rows
 * with an "expand all" toggle so a server reporting 30 checks doesn't
 * push the rest of the page off-screen. Render nothing when there are no
 * checks to show.
 *
 * Each entry already carries its own `silenced` flag (from the same
 * scoped-policy pass the health rollup uses); the silenced-refs fetch
 * here only feeds the manage buttons and the "silenced at N scope" chip.
 * Splits into a grouped/ungrouped variant only to keep the group-scope
 * silenced-refs fetch off ungrouped servers — `useApi` is unconditional,
 * so a single component can't gate the hook on `groupId`. */
function ChecksTable(props: {
	checks: ConsolidatedChecks;
	operators: OperatorPresence[];
	serverId: string;
	groupId: string | null;
	maintained: boolean;
	refreshTick: number;
	onSilenced: () => void;
}) {
	const serverApi = useApi(
		"silenced_refs",
		"list_for_server",
		{ server_id: props.serverId },
		[props.serverId, props.refreshTick],
	);
	const serverSilences =
		serverApi.status === "ok" ? serverApi.data : [];
	if (props.groupId) {
		return (
			<ChecksTableGrouped
				{...props}
				groupId={props.groupId}
				serverSilences={serverSilences}
			/>
		);
	}
	return (
		<ChecksTableBody
			{...props}
			serverSilences={serverSilences}
			groupSilences={[]}
		/>
	);
}

function ChecksTableGrouped(props: {
	checks: ConsolidatedChecks;
	operators: OperatorPresence[];
	serverId: string;
	groupId: string;
	maintained: boolean;
	refreshTick: number;
	onSilenced: () => void;
	serverSilences: ServerSilencedRef[];
}) {
	const groupApi = useApi(
		"silenced_refs",
		"list_for_group",
		{ server_group_id: props.groupId },
		[props.groupId, props.refreshTick],
	);
	const groupSilences = groupApi.status === "ok" ? groupApi.data : [];
	return <ChecksTableBody {...props} groupSilences={groupSilences} />;
}

function ChecksTableBody({
	checks,
	operators,
	serverId,
	groupId,
	maintained,
	onSilenced,
	serverSilences,
	groupSilences,
}: {
	checks: ConsolidatedChecks;
	operators: OperatorPresence[];
	serverId: string;
	groupId: string | null;
	maintained: boolean;
	onSilenced: () => void;
	serverSilences: ServerSilencedRef[];
	groupSilences: ServerGroupSilencedRef[];
}) {
	const entries = checks.checks;
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
				{visible.map((entry) => {
					// Match the silence refs to this entry's own source — a
					// silence on another source's same-named check is a
					// different check, and canopy's own checks are silenced
					// at a bare ref rather than under `health/`.
					const refName = silenceRef(entry.source, entry.check);
					const serverSilence =
						serverSilences.find(
							(s) => s.source === entry.source && s.ref === refName,
						) ?? null;
					const groupSilence =
						groupSilences.find(
							(s) => s.source === entry.source && s.ref === refName,
						) ?? null;
					return (
						<CheckRow
							key={`${entry.source}:${entry.check}`}
							entry={entry}
							operators={operators}
							serverId={serverId}
							groupId={groupId}
							maintained={maintained}
							onSilenced={onSilenced}
							serverSilence={serverSilence}
							groupSilence={groupSilence}
						/>
					);
				})}
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

function CheckRow({
	entry,
	operators,
	serverId,
	groupId,
	maintained,
	onSilenced,
	serverSilence,
	groupSilence,
}: {
	entry: ConsolidatedCheck;
	operators: OperatorPresence[];
	serverId: string;
	groupId: string | null;
	maintained: boolean;
	onSilenced: () => void;
	serverSilence: ServerSilencedRef | null;
	groupSilence: ServerGroupSilencedRef | null;
}) {
	const isAdmin = useIsAdmin() === true;
	// `external_users` gets a formatted session list instead of the raw
	// `users` JSON; the headline `count` is subsumed by it too. Falls
	// through to the generic dl when the payload shape is unexpected.
	const allExtras = checkEntryExtras(
		(entry.detail ?? {}) as Record<string, unknown>,
	);
	const sessions =
		entry.check === "external_users"
			? parseExternalUserSessions(allExtras)
			: null;
	const extras =
		sessions === null
			? allExtras
			: allExtras.filter(([k]) => k !== "users" && k !== "count");
	const effective = entry.effective as CheckResult;
	const quiet =
		entry.silenced || effective === "passed" || effective === "skipped";
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
				bgcolor: quiet ? undefined : "action.hover",
			}}
		>
			<CheckResultIcon
				observed={entry.observed as CheckResult | null}
				effective={effective}
				silenced={entry.silenced}
			/>
			<Box sx={{ flex: 1, minWidth: 0 }}>
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center", flexWrap: "wrap" }}
					useFlexGap
				>
					<Typography variant="body2" sx={{ fontFamily: "monospace" }}>
						<MuiLink component={RouterLink} to={healthcheckPath(entry.source, entry.check)}>
							{entry.check}
						</MuiLink>
					</Typography>
					<Typography variant="caption" color="text.secondary">
						{entry.source}
					</Typography>
					<CheckDocButton source={entry.source} check={entry.check} />
					<SilencedChip
						serverSilence={serverSilence}
						groupSilence={groupSilence}
					/>
					{maintained && effective === "skipped" && (
						<MaintenanceSkipChip />
					)}
				</Stack>
				{sessions !== null && (
					<ExternalUsersDetails
						sessions={sessions}
						operators={operators}
					/>
				)}
				<CheckExtrasList extras={extras} />
			</Box>
			{isAdmin && (
				<SilenceCheckButton
					check={entry.check}
					serverId={serverId}
					groupId={groupId}
					source={entry.source}
					onSilenced={onSilenced}
					serverSilence={serverSilence}
					groupSilence={groupSilence}
				/>
			)}
		</Stack>
	);
}

/** Per-check result icon, coloured by the check's *effective* result
 * (what policy grades it to). A silenced check gets the same neutral
 * grey treatment as a skipped one — its result still records, it just
 * doesn't count toward the server's health. When the observed result
 * differs from the effective one, the tooltip notes the grading. */
function CheckResultIcon({
	observed,
	effective,
	silenced = false,
}: {
	observed: CheckResult | null;
	effective: CheckResult;
	silenced?: boolean;
}) {
	if (silenced) {
		return (
			<Tooltip
				title={`Silenced — reported ${observed ?? "?"}, not counted toward server health`}
				arrow
			>
				<NotificationsOffIcon fontSize="small" color="disabled" />
			</Tooltip>
		);
	}
	const DESCRIPTION: Record<CheckResult, string> = {
		passed: "Passing",
		warning: "Warning — degraded but not failing",
		failed: "Failing",
		broken: "Broken — the check itself is failing, not the system under test",
		skipped: "Skipped — a precondition was not met",
	};
	const tooltip =
		observed && observed !== effective
			? `${DESCRIPTION[effective]} (reported ${observed}, graded ${effective})`
			: DESCRIPTION[effective];
	switch (effective) {
		case "passed":
			return (
				<Tooltip title={tooltip} arrow>
					<CheckCircleIcon fontSize="small" color="success" />
				</Tooltip>
			);
		case "warning":
			return (
				<Tooltip title={tooltip} arrow>
					<WarningAmberIcon fontSize="small" color="warning" />
				</Tooltip>
			);
		case "failed":
			return (
				<Tooltip title={tooltip} arrow>
					<CancelIcon fontSize="small" color="error" />
				</Tooltip>
			);
		case "broken":
			return (
				<Tooltip title={tooltip} arrow>
					<BuildCircleIcon fontSize="small" color="warning" />
				</Tooltip>
			);
		case "skipped":
			return (
				<Tooltip title={tooltip} arrow>
					<RemoveCircleOutlinedIcon fontSize="small" color="disabled" />
				</Tooltip>
			);
	}
}

/** Inline indicator showing that a check's `(status, health/<check>)` ref
 * is already in the silence list at one or both scopes. Shown for all
 * viewers (silences are listable without admin); the row's silence
 * button still gates the manage actions on admin. */
/** Why a check under a window graded to skipped. Without it the row reads
 * as a precondition the check itself did not meet.
 * spec: MNT#presentation */
function MaintenanceSkipChip() {
	return (
		<Tooltip title="A maintenance window holds here, so every check on this server grades to skipped and raises nothing.">
			<Chip
				size="small"
				variant="outlined"
				icon={<BuildOutlinedIcon />}
				label="skipped: under maintenance"
				data-testid="check-maintenance-skip"
			/>
		</Tooltip>
	);
}

function SilencedChip({
	serverSilence,
	groupSilence,
}: {
	serverSilence: ServerSilencedRef | null;
	groupSilence: ServerGroupSilencedRef | null;
}) {
	if (!serverSilence && !groupSilence) return null;
	const scopes: string[] = [];
	if (serverSilence) scopes.push("server");
	if (groupSilence) scopes.push("group");
	const tooltipLines: string[] = [];
	if (serverSilence) {
		tooltipLines.push(
			`Server-scope silence${
				serverSilence.created_by ? ` by ${serverSilence.created_by}` : ""
			}`,
		);
	}
	if (groupSilence) {
		tooltipLines.push(
			`Group-scope silence${
				groupSilence.created_by ? ` by ${groupSilence.created_by}` : ""
			}`,
		);
	}
	return (
		<Tooltip title={tooltipLines.join(" · ")}>
			<Chip
				size="small"
				variant="outlined"
				icon={<NotificationsOffIcon />}
				label={`silenced (${scopes.join(" + ")})`}
			/>
		</Tooltip>
	);
}

/** Compact silence trigger on each `CheckRow`. Opens a popover that
 * shows, per scope, either the existing silence (with an Un-silence
 * action) or a Silence button. Filled icon + primary colour signals that
 * the row is already silenced at one or both scopes — operators can spot
 * "this check is covered" without opening the popover. On any mutation,
 * calls the parent's `onSilenced` so the `ChecksTable`'s silence fetches
 * and the page's `SilencedRefsSection` refetch in lockstep. */
function SilenceCheckButton({
	check,
	serverId,
	groupId,
	source,
	onSilenced,
	serverSilence,
	groupSilence,
}: {
	check: string;
	serverId: string;
	groupId: string | null;
	source: string;
	onSilenced: () => void;
	serverSilence: ServerSilencedRef | null;
	groupSilence: ServerGroupSilencedRef | null;
}) {
	const silenceServer = useApiAction("silenced_refs", "silence_server");
	const silenceGroup = useApiAction("silenced_refs", "silence_group");
	const unsilenceServer = useApiAction("silenced_refs", "unsilence_server");
	const unsilenceGroup = useApiAction("silenced_refs", "unsilence_group");
	const [anchorEl, setAnchorEl] = useState<HTMLElement | null>(null);
	const error =
		silenceServer.error ??
		silenceGroup.error ??
		unsilenceServer.error ??
		unsilenceGroup.error;
	const refName = silenceRef(source, check);
	const silenced = !!serverSilence || !!groupSilence;
	const handle = async (fn: () => Promise<unknown>) => {
		try {
			await fn();
			onSilenced();
			setAnchorEl(null);
		} catch {
			/* surfaced via error */
		}
	};
	return (
		<>
			<Tooltip
				title={silenced ? "Silenced — manage…" : "Silence this check…"}
			>
				<IconButton
					size="small"
					color={silenced ? "primary" : "default"}
					aria-label={
						silenced
							? `Manage silence for ${check}`
							: `Silence ${check}`
					}
					onClick={(e) => setAnchorEl(e.currentTarget)}
				>
					{silenced ? (
						<NotificationsOffIcon fontSize="small" />
					) : (
						<NotificationsOffOutlinedIcon fontSize="small" />
					)}
				</IconButton>
			</Tooltip>
			<Popover
				open={!!anchorEl}
				anchorEl={anchorEl}
				onClose={() => setAnchorEl(null)}
				anchorOrigin={{ vertical: "bottom", horizontal: "right" }}
				transformOrigin={{ vertical: "top", horizontal: "right" }}
			>
				<Box sx={{ p: 1.5, maxWidth: 360 }}>
					<Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
						Permanently ignore <code>
							{source}/{refName}
						</code>
						. The check still records, but no longer triggers or joins
						incidents.
					</Typography>
					<Stack spacing={0.75}>
						<SilenceScopeRow
							scopeLabel="this server"
							silence={serverSilence}
							onSilence={() =>
								handle(() =>
									silenceServer.call({
										server_id: serverId,
										source,
										ref: refName,
									}),
								)
							}
							onUnsilence={() =>
								handle(() =>
									unsilenceServer.call({
										server_id: serverId,
										source,
										ref: refName,
									}),
								)
							}
						/>
						{groupId && (
							<SilenceScopeRow
								scopeLabel="this group"
								silence={groupSilence}
								onSilence={() =>
									handle(() =>
										silenceGroup.call({
											server_group_id: groupId,
											source,
											ref: refName,
										}),
									)
								}
								onUnsilence={() =>
									handle(() =>
										unsilenceGroup.call({
											server_group_id: groupId,
											source,
											ref: refName,
										}),
									)
								}
							/>
						)}
					</Stack>
					{error && (
						<Alert severity="error" sx={{ mt: 1 }}>
							{error.message}
						</Alert>
					)}
				</Box>
			</Popover>
		</>
	);
}

/** One row in the silence-check popover, scoped to either the server or
 * the group. Renders an Un-silence button (with provenance) when the
 * scope already has a silence for this ref, or a Silence button when it
 * doesn't. */
function SilenceScopeRow({
	scopeLabel,
	silence,
	onSilence,
	onUnsilence,
}: {
	scopeLabel: string;
	silence: { created_at: string; created_by: string | null } | null;
	onSilence: () => void;
	onUnsilence: () => void;
}) {
	if (silence) {
		return (
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<Typography variant="caption" sx={{ flex: 1, minWidth: 0 }}>
					Silenced for {scopeLabel}
					<Box component="span" sx={{ color: "text.secondary" }}>
						{" — "}
						<TimeAgo timestamp={silence.created_at} />
						{silence.created_by && ` by ${silence.created_by}`}
					</Box>
				</Typography>
				<Button
					size="small"
					variant="outlined"
					startIcon={<NotificationsActiveOutlinedIcon />}
					onClick={onUnsilence}
				>
					Un-silence
				</Button>
			</Stack>
		);
	}
	return (
		<Button
			size="small"
			variant="outlined"
			startIcon={<NotificationsOffOutlinedIcon />}
			onClick={onSilence}
			sx={{ alignSelf: "flex-start" }}
		>
			For {scopeLabel}
		</Button>
	);
}

function StatusInfoFields({ status }: { status: ServerLastStatusData }) {
	const tracking = useProductCaps(status.product)?.version_tracking;
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
			{/* A product with no application version shows no version block at
			    all — the caption included, since an empty one reads as a
			    reporting failure rather than an absence.
			    spec: APP#versions */}
			{tracking !== undefined && tracking !== "absent" && (
				<Stack spacing={0.25}>
					<Typography variant="caption" color="text.secondary">
						{PRODUCT_LABELS[status.product]}
					</Typography>
					<VersionIndicator
						version={status.version}
						tracking={tracking}
						distance={status.version_distance}
					/>
				</Stack>
			)}
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
			{status.bestool && (
				<InfoItem label="bestool" value={status.bestool} mono />
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

/// What this server is trusted to do with names under its group's domains. Only
/// called where it is trusted with one of them, so there is no "none" reading.
// spec: DOM#permission-for-a-server-to-manage-its-own-names
function nameManagementLabel(server: ServerInfo): string {
	if (server.may_manage_dns && server.may_manage_tls) return "DNS and TLS";
	return server.may_manage_dns ? "DNS only" : "TLS only";
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
	// Same rank-then-kind grouping as the GroupDetail server list, so a
	// reader scanning ServerDetail's sibling section sees the production
	// peers up top and the dev scratch at the bottom in a predictable
	// order. Unranked servers fall into a trailing `null` bucket.
	const groups = groupServersByRank(siblings);

	return (
		<Box>
			<Typography variant="h5" component="h2" gutterBottom>
				Other servers in this group ({siblings.length})
			</Typography>
			<Stack spacing={2}>
				{groups.map(([rank, members]) => (
					<Box key={rank ?? "_unranked"}>
						{rank && (
							<Typography
								variant="overline"
								color="text.secondary"
								sx={{ display: "block", mb: 0.5 }}
							>
								{rank}
							</Typography>
						)}
						<Stack spacing={1}>
							{members.map((sib) => (
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
									{sib.display_host && (
										<Tooltip title={sib.display_host}>
											<IconButton
												component="a"
												href={sib.display_host}
												target="_blank"
												rel="noopener noreferrer"
												size="small"
												aria-label={`Open ${sib.name ?? "server"} (${sib.display_host})`}
											>
												<LanguageIcon fontSize="small" />
											</IconButton>
										</Tooltip>
									)}
									<StatusDot
										up={sib.up ?? "gone"}
										health={sib.health ?? undefined}
										monitored={sib.is_monitored !== false}
										maintained={sib.maintained === true}
									/>
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
									{!sib.is_monitored && (
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
				))}
			</Stack>
		</Box>
	);
}

/// The all-zero group id, used to query backup config for an ungrouped server:
/// it always resolves to "no config" rather than erroring on a missing group.
const NIL_UUID = "00000000-0000-0000-0000-000000000000";

/// Per-(server, type) backup capabilities with an admin-only enable toggle.
/// Reads `backups.capabilities`; the switch calls `backups.set_capability` and
/// refetches. Capabilities are advertised by bestool, so a server with none yet
/// renders an explicit empty state rather than disappearing. When the group has
/// no active backup config the toggles are greyed + collapsed behind a message,
/// since they have no effect until backups are set up.
function BackupCapabilitiesSection({
	serverId,
	groupId,
	isAdmin,
}: {
	serverId: string;
	groupId: string | null;
	isAdmin: boolean;
}) {
	// Poll faster while a backup of any type is in flight, so its figures advance
	// and the "backing up…" chip appears and clears on its own; back off when
	// nothing is running. Without this the section was frozen at page load.
	const [inFlight, setInFlight] = useState(false);
	const backupTick = useReloadInterval(
		inFlight ? 5_000 : 30_000,
		"canopy-data-changed",
	);
	const caps = useApi(
		"backups",
		"capabilities",
		{ server_id: serverId },
		[serverId, backupTick],
	);
	const anyInFlight =
		caps.status === "ok" && caps.data.some((c) => c.processing_since != null);
	useEffect(() => {
		setInFlight(anyInFlight);
	}, [anyInFlight]);
	// Whether the group has an *active* (ready) backup config. Ungrouped servers
	// query the nil group, which always returns no config. While this is loading
	// we optimistically treat the section as active to avoid a grey→normal flash.
	const config = useApi(
		"backups",
		"get",
		{ server_group_id: groupId ?? NIL_UUID },
		[groupId],
	);
	// The server's restore window: while open, an operator can run an ad-hoc
	// `bestool canopy restore` on the box. Server-scoped, so it's shown here as
	// well as on the group backups page.
	const restore = useApi(
		"backups",
		"restore_window",
		{ server_id: serverId },
		[serverId],
	);
	const allowRestore = useApiAction("backups", "allow_restore");
	const disallowRestore = useApiAction("backups", "disallow_restore");
	const restoreUntil =
		restore.status === "ok" ? restore.data.allowed_until : null;
	const onAllowRestore = async () => {
		try {
			await allowRestore.call({ server_id: serverId });
			restore.reload();
		} catch {
			/* surfaced via allowRestore.error */
		}
	};
	const onDisallowRestore = async () => {
		try {
			await disallowRestore.call({ server_id: serverId });
			restore.reload();
		} catch {
			/* surfaced via disallowRestore.error */
		}
	};

	const inactive =
		config.status === "ok" &&
		!(config.data != null && config.data.status === "ready");
	const inactiveMessage = !groupId
		? "This server isn't in a group, so backups can't be configured for it."
		: config.status === "ok" && config.data == null
			? "Backups aren't set up for this group yet, so these settings have no effect."
			: "Backups for this group are still being set up, so these settings have no effect yet.";

	const [showInactive, setShowInactive] = useState(false);

	const rows = (
		<Stack divider={<Divider />}>
			{caps.status === "ok" &&
				caps.data.map((cap) => (
					<BackupCapabilityRow
						key={cap.type}
						serverId={serverId}
						cap={cap}
						isAdmin={isAdmin}
						onChanged={caps.reload}
					/>
				))}
		</Stack>
	);

	return (
		<Paper id="backups" variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "baseline", justifyContent: "space-between" }}
			>
				<Typography variant="h6" component="h2" gutterBottom>
					Backups
				</Typography>
				{groupId && (
					<MuiLink
						component={RouterLink}
						to={`/groups/${groupId}/backups`}
						variant="body2"
						underline="hover"
					>
						Group backups ›
					</MuiLink>
				)}
			</Stack>

			{groupId && isAdmin && (
				<Box sx={{ mb: 1 }}>
					{restoreUntil ? (
						<Alert
							severity="warning"
							icon={<RestoreDataIcon fontSize="inherit" />}
							action={
								<Button
									color="inherit"
									size="small"
									onClick={onDisallowRestore}
									disabled={allowRestore.pending || disallowRestore.pending}
								>
									Disable
								</Button>
							}
						>
							Restores are allowed for this server until{" "}
							<TimeAgo timestamp={restoreUntil} />. While open, the server can
							restore backups on demand.
						</Alert>
					) : (
						<Button
							size="small"
							color="warning"
							variant="outlined"
							startIcon={<RestoreDataIcon />}
							onClick={onAllowRestore}
							disabled={allowRestore.pending || disallowRestore.pending}
						>
							Allow restores (24h)
						</Button>
					)}
					{(allowRestore.error || disallowRestore.error) && (
						<Alert severity="error" sx={{ mt: 1 }}>
							{(allowRestore.error || disallowRestore.error)!.message}
						</Alert>
					)}
				</Box>
			)}

			{inactive && (
				<Alert
					severity="info"
					sx={{ mb: 1 }}
					action={
						groupId && isAdmin ? (
							<Button
								component={RouterLink}
								to={`/groups/${groupId}/backups`}
								color="inherit"
								size="small"
							>
								Set up
							</Button>
						) : undefined
					}
				>
					{inactiveMessage}
				</Alert>
			)}

			{caps.status === "loading" || caps.status === "idle" ? (
				<LinearProgress />
			) : caps.status === "error" ? (
				<Alert severity="error">{caps.error.message}</Alert>
			) : caps.data.length === 0 ? (
				<Typography variant="body2" color="text.secondary">
					No backup types registered for this server.
				</Typography>
			) : inactive ? (
				// Collapsed + greyed: the toggles still work (they record intent for
				// when backups are set up), but it's clear they're dormant right now.
				<>
					<Button
						size="small"
						onClick={() => setShowInactive((s) => !s)}
						endIcon={
							<ExpandMoreIcon
								sx={{
									transform: showInactive ? "rotate(180deg)" : "none",
									transition: "transform 150ms",
								}}
							/>
						}
					>
						{showInactive
							? "Hide backup types"
							: `Show backup types (${caps.data.length})`}
					</Button>
					<Collapse in={showInactive}>
						<Box sx={{ opacity: 0.6, mt: 1 }}>{rows}</Box>
					</Collapse>
				</>
			) : (
				rows
			)}
		</Paper>
	);
}

function BackupCapabilityRow({
	serverId,
	cap,
	isAdmin,
	onChanged,
}: {
	serverId: string;
	cap: ServerBackupCapabilityView;
	isAdmin: boolean;
	onChanged: () => void;
}) {
	const setCapability = useApiAction("backups", "set_capability");
	const onToggle = async (enabled: boolean) => {
		try {
			await setCapability.call({
				server_id: serverId,
				type: cap.type,
				enabled,
			});
			onChanged();
		} catch {
			/* surfaced via setCapability.error */
		}
	};
	return (
		<Stack
			direction="row"
			spacing={2}
			sx={{ alignItems: "center", justifyContent: "space-between", py: 0.5 }}
		>
			<Stack spacing={0.25} sx={{ minWidth: 0 }}>
				<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
					<Typography variant="body2" sx={{ fontFamily: "monospace" }}>
						{cap.type}
					</Typography>
					<BackupProcessingChip since={cap.processing_since} />
				</Stack>
				<BackupLiveProgress progress={cap.progress} />
				<LatestSnapshot
					id={cap.latest_snapshot_id}
					at={cap.latest_snapshot_at}
					bytes={cap.latest_snapshot_bytes}
				/>
			</Stack>
			<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
				{setCapability.error && (
					<Typography variant="caption" color="error">
						{setCapability.error.message}
					</Typography>
				)}
				<Switch
					checked={cap.enabled}
					disabled={!isAdmin || setCapability.pending}
					onChange={(e) => onToggle(e.target.checked)}
					slotProps={{
						input: { "aria-label": `Enable ${cap.type} backups` },
					}}
				/>
			</Stack>
		</Stack>
	);
}

function GroupSection({
	group,
	billingLabels,
}: {
	group: ServerGroup;
	billingLabels: ServerDetailData["billing_labels"];
}) {
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
			{billingLabels.length > 0 && (
				<>
					<Typography
						variant="caption"
						color="text.secondary"
						sx={{ display: "block", mt: 1 }}
					>
						Billing labels
					</Typography>
					<Stack direction="row" sx={{ flexWrap: "wrap", gap: 0.5 }}>
						{billingLabels.map((t) => (
							<Chip key={t.key} size="small" label={`${t.key}=${t.value}`} />
						))}
					</Stack>
				</>
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

/// Header strip of StatusDots: the focused server (full-colour) plus its
/// siblings (dimmed), sorted by rank then kind. A thin grey vertical bar
/// separates adjacent ranks. The focused server's dot is the only one
/// without `dim`, so it visually pops regardless of where in the strip
/// rank+kind ordering places it.
function SiblingDotStrip({
	focused,
	focusedUp,
	focusedHealth,
	siblings,
}: {
	focused: ServerInfo & { name?: string | null };
	focusedUp: ShortStatus;
	focusedHealth: HealthState;
	siblings: Array<
		ServerInfo & {
			up?: ShortStatus | null;
			health?: HealthState | null;
			name?: string | null;
		}
	>;
}) {
	// Combine + sort. The focused server keeps a marker so we can render
	// it without `dim` once the order is established.
	const combined: Array<{
		entry: ServerInfo & {
			up?: ShortStatus | null;
			health?: HealthState | null;
		};
		focused: boolean;
	}> = [
		{
			entry: { ...focused, up: focusedUp, health: focusedHealth },
			focused: true,
		},
		...siblings.map((sib) => ({ entry: sib, focused: false })),
	];
	combined.sort((a, b) => compareServersByRankThenKind(a.entry, b.entry));

	const chunks: Array<{
		rank: string;
		entries: typeof combined;
	}> = [];
	for (const m of combined) {
		const key = m.entry.rank ?? "_unranked";
		const last = chunks[chunks.length - 1];
		if (last && last.rank === key) last.entries.push(m);
		else chunks.push({ rank: key, entries: [m] });
	}

	return (
		<Box component="span" sx={{ display: "inline-flex", alignItems: "center" }}>
			{chunks.map((chunk, idx) => (
				<Box
					key={chunk.rank}
					component="span"
					sx={{ display: "inline-flex", alignItems: "center" }}
				>
					{idx > 0 && (
						<Box
							component="span"
							aria-hidden
							sx={{
								display: "inline-block",
								width: "2px",
								height: "0.7em",
								mx: 0.5,
								bgcolor: "text.disabled",
							}}
						/>
					)}
					{chunk.entries.map((m) => (
						<StatusDot
							key={m.entry.id}
							up={(m.entry.up as ShortStatus | undefined) ?? "gone"}
							health={(m.entry.health as HealthState | undefined) ?? undefined}
							monitored={m.entry.is_monitored !== false}
							maintained={m.entry.maintained === true}
							title={m.entry.name ?? ""}
							dim={!m.focused}
							size="0.8em"
						/>
					))}
				</Box>
			))}
		</Box>
	);
}
