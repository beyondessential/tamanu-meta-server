import { Alert, Button, LinearProgress, Stack } from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import { Link as RouterLink } from "react-router-dom";
import GroupShorty from "../components/GroupShorty";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

export default function GroupsList() {
	usePageTitle("Server groups");
	const groups = useApi("server_groups", "list", {}, []);
	const isAdmin = useApi("commons", "is_current_user_admin");
	const admin = isAdmin.status === "ok" && isAdmin.data;

	if (groups.status === "loading" || groups.status === "idle") {
		return <LinearProgress />;
	}
	if (groups.status === "error") {
		return <Alert severity="error">{groups.error.message}</Alert>;
	}

	return (
		<Stack spacing={2}>
			{admin && (
				<Stack direction="row" spacing={1} sx={{ justifyContent: "flex-end" }}>
					<Button
						component={RouterLink}
						to="/groups/new"
						variant="outlined"
						startIcon={<AddIcon />}
					>
						New group
					</Button>
					<Button
						component={RouterLink}
						to="/servers/new"
						variant="contained"
						startIcon={<AddIcon />}
					>
						Add server
					</Button>
				</Stack>
			)}
			{groups.data.length === 0 ? (
				<Alert severity="info">
					No server groups yet. Create one above, or add a server and assign
					it to a new group.
				</Alert>
			) : (
				<Stack spacing={1}>
					{groups.data.map((g) => (
						<GroupShorty key={g.id} group={g} />
					))}
				</Stack>
			)}
		</Stack>
	);
}
