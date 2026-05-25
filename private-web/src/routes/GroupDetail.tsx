import {
	Alert,
	Box,
	Button,
	Chip,
	LinearProgress,
	Paper,
	Stack,
	Typography,
} from "@mui/material";
import EditIcon from "@mui/icons-material/Edit";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { Link as RouterLink, useParams } from "react-router-dom";
import ServerShorty from "../components/ServerShorty";
import SilencedRefsSection from "../components/SilencedRefsSection";
import TimeAgo from "../components/TimeAgo";
import { useApi } from "../api";
import { useIsNotificationHeld } from "../hooks/useIsNotificationHeld";
import { usePageTitle } from "../hooks/usePageTitle";
import type { IncidentData } from "../types";

export default function GroupDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const detail = useApi("server_groups", "get", { server_group_id: id }, [id]);
	const isAdmin = useApi("commons", "is_current_user_admin");
	// Only the currently-open incident matters for the active-incident
	// section; closed ones live behind the /incidents filter route.
	const activeIncidents = useApi(
		"incidents",
		"list_for_group",
		{ server_group_id: id, include_closed: false, limit: 1 },
		[id],
	);
	usePageTitle(detail.status === "ok" ? detail.data.group.name : "Group");

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <Alert severity="error">{detail.error.message}</Alert>;
	}

	const { group, servers } = detail.data;
	const admin = isAdmin.status === "ok" && isAdmin.data;
	const tagEntries = Object.entries(group.tags ?? {});
	const openIncident =
		activeIncidents.status === "ok" && activeIncidents.data.length > 0
			? activeIncidents.data[0]
			: null;

	return (
		<Stack spacing={3}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h4" component="h1">
					{group.name}
				</Typography>
				{admin && (
					<Button
						component={RouterLink}
						to={`/groups/${group.id}/edit`}
						variant="contained"
						startIcon={<EditIcon />}
					>
						Edit
					</Button>
				)}
			</Stack>

			{openIncident && <ActiveIncidentCard incident={openIncident} />}

			{group.notes && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Typography variant="h6" component="h2" gutterBottom>
						Notes
					</Typography>
					<Typography variant="body2" sx={{ whiteSpace: "pre-wrap" }}>
						{group.notes}
					</Typography>
				</Paper>
			)}

			{tagEntries.length > 0 && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Typography variant="h6" component="h2" gutterBottom>
						Tags
					</Typography>
					<Box sx={{ display: "flex", flexWrap: "wrap", gap: 0.5 }}>
						{tagEntries.map(([k, v]) => (
							<Chip
								key={k}
								size="small"
								variant="outlined"
								label={`${k}=${v}`}
							/>
						))}
					</Box>
				</Paper>
			)}

			<Box>
				<Typography variant="h5" component="h2" gutterBottom>
					Servers ({servers.length})
				</Typography>
				{servers.length === 0 ? (
					<Alert severity="info">
						No servers in this group yet. Edit a server and pick this group
						from the group selector.
					</Alert>
				) : (
					<Stack spacing={1}>
						{servers.map((s) => (
							<ServerShorty key={s.id} server={s} />
						))}
					</Stack>
				)}
			</Box>

			<SilencedRefsSection scope="group" id={group.id} />
		</Stack>
	);
}

function ActiveIncidentCard({ incident }: { incident: IncidentData }) {
	const held = useIsNotificationHeld(incident.notification_held_until);
	return (
		<Paper
			variant="outlined"
			sx={{
				p: 2,
				borderColor: held ? "warning.main" : "error.main",
				borderWidth: 2,
			}}
		>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Stack direction="row" spacing={1.5} sx={{ alignItems: "center" }}>
					<WarningAmberIcon color={held ? "warning" : "error"} />
					<Box>
						<Typography variant="h6" component="h2">
							Active incident
							<Box
								component="span"
								sx={{
									ml: 1,
									fontFamily: "monospace",
									color: "text.secondary",
									fontWeight: "normal",
									fontSize: "0.85em",
								}}
							>
								{incident.id.slice(0, 8)}
							</Box>
						</Typography>
						<Typography variant="body2" color="text.secondary">
							opened <TimeAgo timestamp={incident.opened_at} />
							{held && incident.notification_held_until && (
								<>
									{" · "}
									<Box component="span" sx={{ color: "warning.main" }}>
										Slack notice held; ships{" "}
										<TimeAgo
											timestamp={incident.notification_held_until}
										/>
									</Box>
								</>
							)}
						</Typography>
					</Box>
				</Stack>
				<Button
					component={RouterLink}
					to={`/incidents/${incident.id}`}
					variant="outlined"
					color={held ? "warning" : "error"}
				>
					Open
				</Button>
			</Stack>
		</Paper>
	);
}
