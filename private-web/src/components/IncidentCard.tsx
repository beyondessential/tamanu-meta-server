import { Box, Stack, Tooltip, Typography } from "@mui/material";
import BugReportIcon from "@mui/icons-material/BugReport";
import NotesIcon from "@mui/icons-material/StickyNote2";
import TimelineIcon from "@mui/icons-material/Timeline";
import { Link as RouterLink } from "react-router-dom";
import TimeAgo from "./TimeAgo";
import UserAvatar from "./UserAvatar";
import type { IncidentData } from "../types";

function serverLabel(name: string | null, host: string): string {
	if (name && name.trim() !== "") return name;
	if (host && host.trim() !== "") return host;
	return "(unknown)";
}

/** Compact, non-interactive view of an open incident. Operators have to
 * click through to the server page to take action. Header carries the
 * acker's avatar (top-right) and the body has a stats row (bottom-right)
 * with issue / event / note counts. */
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
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "flex-start", justifyContent: "space-between" }}
			>
				<Box sx={{ minWidth: 0, flex: 1 }}>
					<Typography variant="subtitle1" sx={{ fontWeight: 500 }} noWrap>
						{serverLabel(incident.server_name, incident.server_host)}
					</Typography>
					<Typography variant="body2" color="text.secondary">
						opened <TimeAgo timestamp={incident.opened_at} />
					</Typography>
				</Box>
				{incident.acknowledged_at && (
					<UserAvatar
						login={incident.acknowledged_by}
						name={incident.acknowledged_by_name}
						profilePic={incident.acknowledged_by_pic}
					/>
				)}
			</Stack>
			<Stack
				direction="row"
				spacing={1.5}
				sx={{ mt: 1, justifyContent: "flex-end", alignItems: "center" }}
			>
				<Stat
					icon={<BugReportIcon fontSize="inherit" />}
					value={incident.issue_count}
					title={`${incident.issue_count} issue${incident.issue_count === 1 ? "" : "s"}`}
				/>
				<Stat
					icon={<TimelineIcon fontSize="inherit" />}
					value={incident.event_count}
					title={`${incident.event_count} event${incident.event_count === 1 ? "" : "s"}`}
				/>
				<Stat
					icon={<NotesIcon fontSize="inherit" />}
					value={incident.note_count}
					title={`${incident.note_count} note${incident.note_count === 1 ? "" : "s"}`}
				/>
			</Stack>
		</Box>
	);
}

function Stat({
	icon,
	value,
	title,
}: {
	icon: React.ReactNode;
	value: number;
	title: string;
}) {
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
