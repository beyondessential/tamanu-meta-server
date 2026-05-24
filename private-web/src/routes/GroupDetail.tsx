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
import { Link as RouterLink, useParams } from "react-router-dom";
import ServerShorty from "../components/ServerShorty";
import SilencedRefsSection from "../components/SilencedRefsSection";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

export default function GroupDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const detail = useApi("server_groups", "get", { server_group_id: id }, [id]);
	const isAdmin = useApi("commons", "is_current_user_admin");
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
