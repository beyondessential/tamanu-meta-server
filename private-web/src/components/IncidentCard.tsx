import { Box, Stack, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import TimeAgo from "./TimeAgo";
import type { IncidentData } from "../types";

function serverLabel(name: string | null, host: string): string {
	if (name && name.trim() !== "") return name;
	if (host && host.trim() !== "") return host;
	return "(unknown)";
}

/** Compact, non-interactive view of an open incident. Used on the global
 * Incidents page as a grid cell. Operators have to click through to the
 * server page to take action — keeps "I just saw this and resolved it"
 * one-clicks from being a thing. No expand, no resolve button. */
export default function IncidentCard({ incident }: { incident: IncidentData }) {
	return (
		<Box
			component={RouterLink}
			to={`/servers/${incident.server_id}`}
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "error.main",
				borderRadius: 1,
				textDecoration: "none",
				color: "text.primary",
				display: "block",
				"&:hover": { bgcolor: "action.hover" },
			}}
		>
			<Stack spacing={0.5}>
				<Typography variant="subtitle1" sx={{ fontWeight: 500 }}>
					{serverLabel(incident.server_name, incident.server_host)}
				</Typography>
				<Typography variant="body2" color="text.secondary">
					opened <TimeAgo timestamp={incident.opened_at} />
				</Typography>
				<Stack direction="row" spacing={1}>
					{incident.acknowledged_at && (
						<Typography
							variant="caption"
							color="info.main"
							title={`by ${incident.acknowledged_by ?? "?"}`}
						>
							acked
						</Typography>
					)}
					{incident.resolved_at && (
						<Typography
							variant="caption"
							color="success.main"
							title={`(${incident.resolved_reason ?? "?"}) by ${incident.resolved_by ?? "?"}`}
						>
							resolved
						</Typography>
					)}
				</Stack>
			</Stack>
		</Box>
	);
}
