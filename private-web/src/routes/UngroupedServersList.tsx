import { Alert, LinearProgress, Stack } from "@mui/material";
import ServerShorty from "../components/ServerShorty";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { compareServersByRankThenType } from "../types";

export default function UngroupedServersList() {
	usePageTitle("Ungrouped servers");
	const result = useApi("servers", "list_ungrouped", {}, []);

	if (result.status === "loading" || result.status === "idle") {
		return <LinearProgress />;
	}
	if (result.status === "error") {
		return <Alert severity="error">{result.error.message}</Alert>;
	}
	if (result.data.items.length === 0) {
		return (
			<Alert severity="success">
				No ungrouped servers — every server has been placed in a group.
			</Alert>
		);
	}
	const sorted = [...result.data.items].sort(compareServersByRankThenType);
	return (
		<Stack spacing={1}>
			{sorted.map((s) => (
				<ServerShorty key={s.id} server={s} />
			))}
		</Stack>
	);
}
