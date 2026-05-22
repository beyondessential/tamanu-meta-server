import { Alert, LinearProgress, Stack } from "@mui/material";
import GroupShorty from "../components/GroupShorty";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

export default function GroupsList() {
	usePageTitle("Server groups");
	const groups = useApi("server_groups", "list", {}, []);

	if (groups.status === "loading" || groups.status === "idle") {
		return <LinearProgress />;
	}
	if (groups.status === "error") {
		return <Alert severity="error">{groups.error.message}</Alert>;
	}
	if (groups.data.length === 0) {
		return (
			<Alert severity="info">
				No server groups yet. Edit a server and assign it to a new group, or
				create one from any server's edit page.
			</Alert>
		);
	}

	return (
		<Stack spacing={1}>
			{groups.data.map((g) => (
				<GroupShorty key={g.id} group={g} />
			))}
		</Stack>
	);
}
