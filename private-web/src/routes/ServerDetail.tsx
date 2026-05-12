import {
	Alert,
	Box,
	Button,
	Chip,
	IconButton,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import EditIcon from "@mui/icons-material/Edit";
import LanguageIcon from "@mui/icons-material/Language";
import { useState } from "react";
import { Link as RouterLink, useParams } from "react-router-dom";
import IncidentsLink from "../components/IncidentsLink";
import ManualEventButton from "../components/ManualEventButton";
import StatusDot from "../components/StatusDot";
import TailnetIdentitySection from "../components/TailnetIdentitySection";
import TimeAgo from "../components/TimeAgo";
import VersionIndicator from "../components/VersionIndicator";
import { StatusLegend, VersionLegend } from "../components/Legends";
import ServerKindChip from "../components/ServerKindChip";
import ServerRankChip from "../components/ServerRankChip";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import type {
	DeviceInfo,
	ServerDetailData,
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
	// issues/incidents APIs. Any mutation (manual-event submit, ack/resolve/
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
				deviceInfo={data.device_info}
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
			{data.child_servers.length > 0 && (
				<ChildServers
					children={data.child_servers}
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
						title={data.server.name ?? ""}
						size="0.8em"
					/>
					{data.child_servers.map(([up, child]) => (
						<StatusDot
							key={child.id}
							up={up}
							title={child.name ?? ""}
							dim
							size="0.8em"
						/>
					))}
					{data.server.name ?? "Unnamed"}
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
	deviceInfo,
}: {
	host: string;
	deviceInfo: DeviceInfo | null;
}) {
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
			{deviceInfo && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Typography variant="h6" component="h2" gutterBottom>
						Device
					</Typography>
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
				</Paper>
			)}
		</Stack>
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
					label="Reachability alerts"
					value={server.alert_when_down ? "On" : "Off"}
				/>
				{server.parent_server_id && (
					<Stack spacing={0.25}>
						<Typography variant="caption" color="text.secondary">
							Parent
						</Typography>
						<MuiLink
							component={RouterLink}
							to={`/servers/${server.parent_server_id}`}
							underline="hover"
						>
							{server.parent_server_name ?? server.parent_server_id}
						</MuiLink>
					</Stack>
				)}
				{server.cloud != null && (
					<InfoItem
						label="Location"
						value={renderLocation(server)}
					/>
				)}
			</Stack>
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
				<InfoItem label="Timezone" value={status.timezone} />
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
}: {
	label: string;
	value: string;
	mono?: boolean;
}) {
	return (
		<Stack spacing={0.25}>
			<Typography variant="caption" color="text.secondary">
				{label}
			</Typography>
			<Typography
				variant="body2"
				sx={mono ? { fontFamily: "monospace" } : undefined}
			>
				{value}
			</Typography>
		</Stack>
	);
}

function renderLocation(server: ServerInfo): string {
	if (!server.cloud) return "On premise";
	return "Cloud";
}

function ChildServers({
	children,
	isAdmin,
	hasOpenIncident,
	onEventSubmitted,
}: {
	children: ServerDetailData["child_servers"];
	isAdmin: boolean;
	hasOpenIncident: boolean;
	onEventSubmitted: () => void;
}) {
	return (
		<Box>
			<Typography variant="h5" component="h2" gutterBottom>
				Child servers ({children.length})
			</Typography>
			<Stack spacing={1}>
				{children.map(([up, child]) => (
					<Stack
						key={child.id}
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
						<Tooltip title={child.host}>
							<IconButton
								component="a"
								href={child.host}
								target="_blank"
								rel="noopener noreferrer"
								size="small"
								aria-label={`Open ${child.name ?? "server"} (${child.host})`}
							>
								<LanguageIcon fontSize="small" />
							</IconButton>
						</Tooltip>
						<StatusDot up={up} />
						<MuiLink
							component={RouterLink}
							to={`/servers/${child.id}`}
							underline="hover"
							color="text.primary"
							sx={{ fontWeight: 500 }}
						>
							{child.name ?? "Unnamed"}
						</MuiLink>
						{child.rank && <ServerRankChip rank={child.rank} />}
						<ServerKindChip kind={child.kind} />
						{!child.alert_when_down && (
							<Tooltip title="Reachability alerts are off for this server — canopy isn't watching it.">
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
								serverId={child.id}
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
