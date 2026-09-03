import { Box, Chip, Link as MuiLink, Stack, Tooltip, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import {
	applicationName,
	type ApplicationType,
	type HealthState,
	type ServerRank,
	type ShortStatus,
} from "../types";
import ApplicationTypeChip from "./ApplicationTypeChip";
import ServerNameWithGroup from "./ServerNameWithGroup";
import ServerRankChip from "./ServerRankChip";
import StatusDot from "./StatusDot";

export interface ServerInfo {
	id: string;
	name: string | null;
	host?: string | null;
	display_host: string;
	type: ApplicationType;
	rank: ServerRank | null;
	group_name?: string | null;
	is_monitored?: boolean;
	up?: ShortStatus | null;
	health?: HealthState | null;
}

export default function ServerShorty({
	server,
	current = false,
}: {
	server: ServerInfo;
	/// Whether this row is the page the operator is already on. It is marked in
	/// place rather than omitted, so a tree reads as a map rather than as a list
	/// of everything else, and its name doesn't link back to where they are.
	/// spec: FLT
	current?: boolean;
}) {
	const name = applicationName(server);
	const unmonitored = server.is_monitored === false;
	return (
		<Stack
			direction="row"
			spacing={2}
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
				alignItems: "center",
				bgcolor: current ? "action.hover" : undefined,
			}}
		>
			{server.up && (
				<StatusDot
					up={server.up}
					health={server.health ?? undefined}
					monitored={!unmonitored}
				/>
			)}
			{current ? (
				<Typography
					component="span"
					color="text.secondary"
					sx={{ fontWeight: 500 }}
				>
					<ServerNameWithGroup
						groupName={server.group_name}
						serverName={name}
					/>
				</Typography>
			) : (
				<MuiLink
					component={RouterLink}
					to={`/servers/${server.id}`}
					underline="hover"
					color="text.primary"
					sx={{ fontWeight: 500 }}
				>
					<ServerNameWithGroup
						groupName={server.group_name}
						serverName={name}
					/>
				</MuiLink>
			)}
			{server.rank && <ServerRankChip rank={server.rank} />}
			<ApplicationTypeChip type={server.type} />
			{unmonitored && (
				<Tooltip title="Status alerts are off for this server — canopy isn't watching it.">
					<Chip size="small" variant="outlined" label="unmonitored" />
				</Tooltip>
			)}
			<Box sx={{ ml: "auto" }}>
				<Typography variant="body2" color="text.secondary">
					{server.display_host}
				</Typography>
			</Box>
		</Stack>
	);
}
