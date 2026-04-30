import { Box, Chip, Link as MuiLink, Stack, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import type { ServerKind, ServerRank } from "../types";

const RANK_COLORS: Record<
	ServerRank,
	"error" | "warning" | "info" | "success" | "primary"
> = {
	production: "error",
	clone: "warning",
	demo: "info",
	test: "info",
	dev: "success",
};

const KIND_COLORS: Record<
	ServerKind,
	"primary" | "info" | "default"
> = {
	central: "primary",
	facility: "info",
	canopy: "default",
};

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
			{server.rank && (
				<Chip
					size="small"
					variant="outlined"
					color={RANK_COLORS[server.rank]}
					label={server.rank}
				/>
			)}
			<Chip
				size="small"
				variant="outlined"
				color={KIND_COLORS[server.kind]}
				label={server.kind}
			/>
			<Box sx={{ ml: "auto" }}>
				<Typography variant="body2" color="text.secondary">
					{server.host}
				</Typography>
			</Box>
		</Stack>
	);
}
