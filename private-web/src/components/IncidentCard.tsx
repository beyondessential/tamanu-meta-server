import { Box, Stack, Tooltip, Typography } from "@mui/material";
import BugReportIcon from "@mui/icons-material/BugReport";
import NotesIcon from "@mui/icons-material/StickyNote2";
import TimelineIcon from "@mui/icons-material/Timeline";
import { Link as RouterLink } from "react-router-dom";
import TimeAgo from "./TimeAgo";
import type { IncidentData } from "../types";

/** Compact view of an open incident; click-through goes to the incident
 * detail page. The body has a stats row (bottom-right) with issue / event
 * / note counts. */
export default function IncidentCard({ incident }: { incident: IncidentData }) {
	return (
		<Box
			component={RouterLink}
			to={`/incidents/${incident.id}`}
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
			<Box>
				<Typography variant="subtitle1" sx={{ fontWeight: 500 }} noWrap>
					{incident.server_group_name || "(unknown group)"}
				</Typography>
				<Typography variant="body2" color="text.secondary">
					opened <TimeAgo timestamp={incident.opened_at} />
				</Typography>
			</Box>
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
				<Stack direction="row" spacing={1.5} sx={{ alignItems: "center" }}>
					<Stat
						icon={<BugReportIcon fontSize="inherit" />}
						value={incident.issue_count ?? 0}
						noun="issue"
					/>
					<Stat
						icon={<TimelineIcon fontSize="inherit" />}
						value={incident.event_count ?? 0}
						noun="event"
					/>
					<Stat
						icon={<NotesIcon fontSize="inherit" />}
						value={incident.note_count ?? 0}
						noun="note"
					/>
				</Stack>
			</Stack>
		</Box>
	);
}

function Stat({
	icon,
	value,
	noun,
}: {
	icon: React.ReactNode;
	value: number;
	noun: string;
}) {
	const title = `${value} ${noun}${value === 1 ? "" : "s"}`;
	return (
		<Tooltip title={title}>
			<Stack
				direction="row"
				spacing={0.5}
				sx={{ alignItems: "center", color: "text.secondary", fontSize: "0.875rem" }}
			>
				{icon}
				<Box component="span">{value}</Box>
			</Stack>
		</Tooltip>
	);
}
