import {
	Alert as MuiAlert,
	Chip,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Stack,
	Typography,
} from "@mui/material";
import { Link as RouterLink, useParams } from "react-router-dom";
import { useApi } from "../api";
import Markdown from "../components/Markdown";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";

/** Read-only view of one manual incident: a support-recorded record,
 * written and edited over the MCP interface rather than in this UI. */
export default function ManualIncidentDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const detail = useApi("manual_incidents", "get", { id }, [id]);

	usePageTitle(detail.status === "ok" ? detail.data.title : "Manual incident");

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <MuiAlert severity="error">{detail.error.message}</MuiAlert>;
	}

	const incident = detail.data;
	const ongoing = incident.ended_at == null;

	return (
		<Stack spacing={3}>
			<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
				<Typography variant="h4" component="h1" sx={{ flex: 1 }}>
					{incident.title}
				</Typography>
				<Chip size="small" variant="outlined" label="manual" />
				{ongoing ? (
					<Chip size="small" color="warning" label="ongoing" />
				) : (
					<Chip size="small" label="ended" />
				)}
			</Stack>

			<Typography variant="body2" color="text.secondary">
				{incident.server_group_id ? (
					<MuiLink
						component={RouterLink}
						to={`/groups/${incident.server_group_id}`}
						underline="hover"
					>
						{incident.server_group_name ?? "(unknown group)"}
					</MuiLink>
				) : (
					"Fleet-wide"
				)}{" "}
				· started <TimeAgo timestamp={incident.started_at} />
				{incident.ended_at && (
					<>
						{" "}
						· ended <TimeAgo timestamp={incident.ended_at} />
					</>
				)}{" "}
				· recorded by {incident.created_by}
			</Typography>

			<Paper variant="outlined" sx={{ p: 2 }}>
				{incident.description ? (
					<Markdown>{incident.description}</Markdown>
				) : (
					<Typography variant="body2" color="text.secondary">
						No description recorded.
					</Typography>
				)}
			</Paper>

			<Typography variant="caption" color="text.secondary">
				Manual incidents are recorded and edited through the MCP interface;
				this page is display-only. Last changed{" "}
				<TimeAgo timestamp={incident.updated_at} />.
			</Typography>
		</Stack>
	);
}
