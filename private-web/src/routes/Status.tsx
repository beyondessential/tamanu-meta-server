import {
	Alert,
	Box,
	Card,
	CardContent,
	Chip,
	LinearProgress,
	Link as MuiLink,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import StatusDot from "../components/StatusDot";
import VersionIndicator from "../components/VersionIndicator";
import { StatusLegend, VersionLegend } from "../components/Legends";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { type CentralServerCard, SERVER_RANK_ORDER } from "../types";

export default function Status() {
	usePageTitle("Status");
	const tick = useReloadInterval(60_000, "canopy-reload-status");
	const incidents = useApi("incidents", "list_active", {}, [tick]);
	const openIncidentServers = new Set<string>(
		incidents.status === "ok" ? incidents.data.map((i) => i.server_id) : [],
	);
	return (
		<Stack spacing={3}>
			<ReleaseSummary tick={tick} />
			<ServerCards tick={tick} openIncidentServers={openIncidentServers} />
			<Box>
				<VersionLegend />
				<Box sx={{ mt: 1 }}>
					<StatusLegend />
				</Box>
			</Box>
		</Stack>
	);
}

function ReleaseSummary({ tick }: { tick: number }) {
	const result = useApi("statuses", "summary", {}, [tick]);
	if (result.status === "loading" || result.status === "idle") {
		return <LinearProgress />;
	}
	if (result.status === "error") {
		return <Alert severity="error">{result.error.message}</Alert>;
	}
	const { releases, versions, bracket } = result.data;
	return (
		<Card variant="outlined">
			<CardContent>
				<Typography variant="body1">
					{releases.length} release branches in active use:{" "}
					{releases
						.map(([major, minor]) => `${major}.${minor}`)
						.join(", ")}{" "}
					({versions.length} versions: {bracket.min} — {bracket.max})
				</Typography>
			</CardContent>
		</Card>
	);
}

function ServerCards({
	tick,
	openIncidentServers,
}: {
	tick: number;
	openIncidentServers: Set<string>;
}) {
	const grouped = useApi(
		"statuses",
		"server_grouped_ids",
		{},
		[tick],
	);

	if (grouped.status === "loading" || grouped.status === "idle") {
		return <LinearProgress />;
	}
	if (grouped.status === "error") {
		return <Alert severity="error">{grouped.error.message}</Alert>;
	}

	const sections = SERVER_RANK_ORDER.flatMap((rank) => {
		const ids = grouped.data[rank];
		if (!ids || ids.length === 0) return [];
		return [{ rank, ids }];
	});

	if (sections.length === 0) {
		return (
			<Alert severity="info">
				No servers configured. Add some via the Servers page.
			</Alert>
		);
	}

	return (
		<Stack spacing={3}>
			{sections.map(({ rank, ids }) => (
				<Box key={rank}>
					<Typography
						variant="h5"
						component="h2"
						sx={{ textTransform: "capitalize", mb: 1 }}
					>
						{rank}
					</Typography>
					<Box
						sx={{
							display: "grid",
							gridTemplateColumns: "repeat(auto-fill, minmax(18em, 1fr))",
							gap: 1.5,
						}}
					>
						{ids.map((id) => (
							<ServerCardLoader
								key={id}
								serverId={id}
								tick={tick}
								hasOpenIncident={openIncidentServers.has(id)}
							/>
						))}
					</Box>
				</Box>
			))}
		</Stack>
	);
}

function ServerCardLoader({
	serverId,
	tick,
	hasOpenIncident,
}: {
	serverId: string;
	tick: number;
	hasOpenIncident: boolean;
}) {
	const result = useApi(
		"statuses",
		"server_details",
		{ server_id: serverId },
		[serverId, tick],
	);

	return (
		<MuiLink
			component={RouterLink}
			to={`/servers/${serverId}`}
			underline="none"
			color="inherit"
		>
			<Card
				variant="outlined"
				sx={{
					transition: "background-color 150ms",
					"&:hover": { bgcolor: "action.hover" },
					...(hasOpenIncident && {
						borderColor: "error.main",
						borderWidth: 2,
					}),
				}}
			>
				<CardContent sx={{ p: 1.5, "&:last-child": { pb: 1.5 } }}>
					{result.status === "loading" || result.status === "idle" ? (
						<Typography variant="body2" color="text.secondary">
							Thinking…
						</Typography>
					) : result.status === "error" ? (
						<Alert severity="error">{result.error.message}</Alert>
					) : (
						<ServerCard
							server={result.data}
							hasOpenIncident={hasOpenIncident}
						/>
					)}
				</CardContent>
			</Card>
		</MuiLink>
	);
}

function ServerCard({
	server,
	hasOpenIncident,
}: {
	server: CentralServerCard;
	hasOpenIncident: boolean;
}) {
	return (
		<Stack spacing={1}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "baseline", justifyContent: "space-between" }}
			>
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "baseline", minWidth: 0 }}
				>
					<Tooltip title={server.host} placement="top">
						<MuiLink
							href={server.host}
							target="_blank"
							rel="noopener"
							color="text.secondary"
							onClick={(e) => e.stopPropagation()}
							sx={{ textDecoration: "none", flexShrink: 0 }}
						>
							🌐
						</MuiLink>
					</Tooltip>
					<Typography
						variant="subtitle1"
						component="h3"
						sx={{
							overflow: "hidden",
							textOverflow: "ellipsis",
							whiteSpace: "nowrap",
							minWidth: 0,
						}}
					>
						{server.name}
					</Typography>
				</Stack>
				<Box sx={{ flexShrink: 0 }}>
					<VersionIndicator
						version={server.version}
						distance={server.version_distance}
						addLink={false}
					/>
				</Box>
			</Stack>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Stack direction="row" spacing={0} sx={{ flexWrap: "wrap" }}>
					<StatusDot up={server.up} title={`${server.name}: ${server.up}`} />
					{server.facility_servers.map((facility) => (
						<StatusDot
							key={facility.id}
							up={facility.up}
							title={`${facility.name}: ${facility.up}`}
							dim
						/>
					))}
				</Stack>
				{hasOpenIncident && (
					<Chip label="incident" color="error" size="small" />
				)}
			</Stack>
		</Stack>
	);
}
