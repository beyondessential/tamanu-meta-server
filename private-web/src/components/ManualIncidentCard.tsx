import { Box, Chip, Stack, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import TimeAgo from "./TimeAgo";
import type { ManualIncidentData } from "../types";

/** Compact view of a manual incident (a support-recorded record, distinct
 * from the automatic incidents canopy opens from check state); click-through
 * goes to the manual incident detail page. Ongoing records get a
 * warning-coloured border; ended ones the neutral divider. */
export default function ManualIncidentCard({
	incident,
}: {
	incident: ManualIncidentData;
}) {
	const ongoing = incident.ended_at == null;
	return (
		<Box
			component={RouterLink}
			to={`/incidents/manual/${incident.id}`}
			sx={{
				p: 1.5,
				border: 1,
				borderColor: ongoing ? "warning.main" : "divider",
				borderRadius: 1,
				textDecoration: "none",
				color: "text.primary",
				display: "block",
				"&:hover": { bgcolor: "action.hover" },
			}}
		>
			<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
				<Typography variant="subtitle1" sx={{ fontWeight: 500, flex: 1 }} noWrap>
					{incident.title}
				</Typography>
				<Chip size="small" variant="outlined" label="manual" />
				{ongoing && <Chip size="small" color="warning" label="ongoing" />}
			</Stack>
			<Typography variant="body2" color="text.secondary">
				{incident.server_group_name ?? "Fleet-wide"} · started{" "}
				<TimeAgo timestamp={incident.started_at} />
				{incident.ended_at && (
					<>
						{" "}
						· ended <TimeAgo timestamp={incident.ended_at} />
					</>
				)}
			</Typography>
			<Stack
				direction="row"
				spacing={1.5}
				sx={{ mt: 1, justifyContent: "space-between", alignItems: "center" }}
			>
				<Typography
					variant="caption"
					color="text.secondary"
					sx={{ fontFamily: "monospace" }}
				>
					{incident.id.slice(0, 8)}
				</Typography>
				<Typography variant="caption" color="text.secondary">
					by {incident.created_by}
				</Typography>
			</Stack>
		</Box>
	);
}
