import { Box, Link as MuiLink, Stack, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import type { ServerKind, ServerRank } from "../types";
import ServerKindChip from "./ServerKindChip";
import ServerRankChip from "./ServerRankChip";

export interface ServerInfo {
	id: string;
	name: string | null;
	host: string;
	kind: ServerKind;
	rank: ServerRank | null;
}

export default function ServerShorty({ server }: { server: ServerInfo }) {
	const name = server.name || "Unnamed server";
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
				{name}
			</MuiLink>
			{server.rank && <ServerRankChip rank={server.rank} />}
			<ServerKindChip kind={server.kind} />
			<Box sx={{ ml: "auto" }}>
				<Typography variant="body2" color="text.secondary">
					{server.host}
				</Typography>
			</Box>
		</Stack>
	);
}
