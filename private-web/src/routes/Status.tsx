import {
	Alert,
	Box,
	Card,
	CardContent,
	LinearProgress,
	Link as MuiLink,
	Stack,
	Typography,
} from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import StatusDot from "../components/StatusDot";
import VersionIndicator from "../components/VersionIndicator";
import { StatusLegend, VersionLegend } from "../components/Legends";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import {
	type CentralServerCard,
	SERVER_RANK_ORDER,
	type ServerGroupedIds,
	type SummaryData,
} from "../types";

export default function Status() {
	usePageTitle("Status");
	const tick = useReloadInterval(60_000, "canopy-reload-status");
	return (
		<Stack spacing={3}>
			<ReleaseSummary tick={tick} />
			<ServerCards tick={tick} />
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
	const result = useApi<SummaryData>("statuses", "summary", {}, [tick]);
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

function ServerCards({ tick }: { tick: number }) {
	const grouped = useApi<ServerGroupedIds>(
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
							<ServerCardLoader key={id} serverId={id} tick={tick} />
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
}: {
	serverId: string;
	tick: number;
}) {
	const result = useApi<CentralServerCard>(
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
						<ServerCard server={result.data} />
					)}
				</CardContent>
			</Card>
		</MuiLink>
	);
}

function ServerCard({ server }: { server: CentralServerCard }) {
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
					sx={{ alignItems: "baseline" }}
				>
					<MuiLink
						href={server.host}
						target="_blank"
						rel="noopener"
						color="text.secondary"
						title={server.host}
						onClick={(e) => e.stopPropagation()}
						sx={{ textDecoration: "none" }}
					>
						🌐
					</MuiLink>
					<Typography
						variant="subtitle1"
						component="h3"
						sx={{
							overflow: "hidden",
							textOverflow: "ellipsis",
							whiteSpace: "nowrap",
						}}
					>
						{server.name}
					</Typography>
				</Stack>
				{server.version && (
					<VersionIndicator
						version={server.version}
						distance={server.version_distance}
						addLink={false}
					/>
				)}
			</Stack>
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
		</Stack>
	);
}
