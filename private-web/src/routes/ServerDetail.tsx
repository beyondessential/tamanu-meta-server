import {
	Alert,
	Box,
	Button,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Stack,
	Typography,
} from "@mui/material";
import EditIcon from "@mui/icons-material/Edit";
import { Link as RouterLink, useParams } from "react-router-dom";
import StatusDot from "../components/StatusDot";
import VersionIndicator from "../components/VersionIndicator";
import { StatusLegend, VersionLegend } from "../components/Legends";
import ServerKindChip from "../components/ServerKindChip";
import ServerRankChip from "../components/ServerRankChip";
import { useApi } from "../api";
import type {
	DeviceShortInfo,
	ServerDetailData,
	ServerInfoFull,
	ServerLastStatusData,
} from "../types";

export default function ServerDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const detail = useApi<ServerDetailData>(
		"servers",
		"get_detail",
		{ server_id: id },
		[id],
	);
	const isAdmin = useApi<boolean>("commons", "is_current_user_admin");

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
			<Header data={data} isAdmin={admin} />
			<UrlAndDevice
				host={data.server.host}
				deviceInfo={data.device_info}
			/>
			<InfoSection
				server={data.server}
				status={data.last_status}
			/>
			{data.child_servers.length > 0 && (
				<ChildServers children={data.child_servers} />
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
}: {
	data: ServerDetailData;
	isAdmin: boolean;
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
					<StatusDot up={data.up} title={data.server.name ?? ""} />
					{data.child_servers.map(([up, child]) => (
						<StatusDot
							key={child.id}
							up={up}
							title={child.name ?? ""}
							dim
						/>
					))}
					{data.server.name ?? "Unnamed"}
				</Typography>
			</Stack>
			{isAdmin && (
				<Button
					component={RouterLink}
					to={`/servers/${data.server.id}/edit`}
					variant="contained"
					startIcon={<EditIcon />}
				>
					Edit
				</Button>
			)}
		</Stack>
	);
}

function UrlAndDevice({
	host,
	deviceInfo,
}: {
	host: string;
	deviceInfo: DeviceShortInfo | null;
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
					<MuiLink
						component={RouterLink}
						to={`/devices/${deviceInfo.device.id}`}
						underline="hover"
					>
						{deviceShortName(deviceInfo)}
					</MuiLink>
				</Paper>
			)}
		</Stack>
	);
}

function deviceShortName(info: DeviceShortInfo): string {
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
	server: ServerInfoFull;
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
			{status && Object.keys(status.extra).length > 0 && (
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
			<InfoItem label="Last seen" value={formatTimestamp(status.created_at)} />
			{status.platform && (
				<InfoItem label="Platform" value={status.platform} />
			)}
			{status.timezone && (
				<InfoItem label="Timezone" value={status.timezone} />
			)}
			{status.version && (
				<Stack spacing={0.25}>
					<Typography variant="caption" color="text.secondary">
						Tamanu
					</Typography>
					<VersionIndicator
						version={status.version}
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

function renderLocation(server: ServerInfoFull): string {
	if (!server.cloud) return "On premise";
	return "Cloud";
}

function formatTimestamp(iso: string): string {
	try {
		return new Date(iso).toLocaleString();
	} catch {
		return iso;
	}
}

function ChildServers({
	children,
}: {
	children: ServerDetailData["child_servers"];
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
						<Box sx={{ ml: "auto" }}>
							<Typography variant="body2" color="text.secondary">
								{child.host}
							</Typography>
						</Box>
					</Stack>
				))}
			</Stack>
		</Box>
	);
}
