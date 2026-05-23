import { Box, Chip, Link as MuiLink, Stack, Tooltip, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import type { ServerKind, ServerRank } from "../types";
import ServerKindChip from "./ServerKindChip";
import ServerNameWithGroup from "./ServerNameWithGroup";
import ServerRankChip from "./ServerRankChip";

export interface ServerInfo {
	id: string;
	name: string | null;
	host: string;
	kind: ServerKind;
	rank: ServerRank | null;
	group_name?: string | null;
	alert_when_down_for?: number;
}

export default function ServerShorty({ server }: { server: ServerInfo }) {
	const name = server.name || "Unnamed server";
	const unmonitored =
		server.alert_when_down_for !== undefined && server.alert_when_down_for <= 0;
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
			}}
		>
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
			{server.rank && <ServerRankChip rank={server.rank} />}
			<ServerKindChip kind={server.kind} />
			{unmonitored && (
				<Tooltip title="Status alerts are off for this server — canopy isn't watching it.">
					<Chip size="small" variant="outlined" label="unmonitored" />
				</Tooltip>
			)}
			<Box sx={{ ml: "auto" }}>
				<Typography variant="body2" color="text.secondary">
					{server.host}
				</Typography>
			</Box>
		</Stack>
	);
}
