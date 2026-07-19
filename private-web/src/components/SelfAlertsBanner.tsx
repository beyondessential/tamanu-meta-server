import { Alert, AlertTitle, Stack } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import { Link } from "@mui/material";
import { useApi } from "../api";
function alertSeverity(result: string | null): "error" | "warning" | "info" {
	switch (result) {
		case "failed":
			return "error";
		case "warning":
		case "broken":
			return "warning";
		default:
			return "info";
	}
}

/// Persistent notice for canopy's own problems (self-alerts), visible from
/// every page while any is active. Distinct from fleet incidents by design:
/// these never belong to a server or group.
export default function SelfAlertsBanner({ reloadTick }: { reloadTick: number }) {
	const active = useApi("self_alerts", "active", {}, [reloadTick]);
	if (active.status !== "ok" || active.data.length === 0) return null;

	return (
		<Stack spacing={1} sx={{ px: 3, pt: 2 }}>
			{active.data.map((alert) => (
				<Alert key={alert.id} severity={alertSeverity(alert.effective_result)}>
					<AlertTitle sx={{ mb: 0 }}>
						Canopy: {alert.title ?? alert.ref}
					</AlertTitle>
					first seen {new Date(alert.first_seen).toLocaleString()} —{" "}
					<Link component={RouterLink} to="/alerts">
						details
					</Link>
				</Alert>
			))}
		</Stack>
	);
}
