import { Alert, Box, CircularProgress, Typography } from "@mui/material";
import { useApi } from "../api";

export default function Hello() {
	const result = useApi<boolean>("commons", "is_current_user_admin");

	return (
		<Box>
			<Typography variant="body1" gutterBottom>
				Hello world. This pings <code>commons.is_current_user_admin</code>{" "}
				through the Vite dev proxy to the private-server.
			</Typography>
			{result.status === "loading" && <CircularProgress size={20} />}
			{result.status === "ok" && (
				<Alert severity={result.data ? "success" : "warning"}>
					Server says you {result.data ? "are" : "are not"} an admin.
				</Alert>
			)}
			{result.status === "error" && (
				<Alert severity="error">
					{result.error.message}
				</Alert>
			)}
		</Box>
	);
}
