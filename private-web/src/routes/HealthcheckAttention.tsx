import {
	Alert,
	Box,
	Chip,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	Typography,
} from "@mui/material";
import { Link as RouterLink, useParams } from "react-router-dom";
import { useApi } from "../api";
import ServerNameWithGroup from "../components/ServerNameWithGroup";
import SeverityChip from "../components/SeverityChip";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { type CheckAttentionServerData } from "../types";

/// MUI chip colour per offending check result. Only warning/failed/broken
/// ever reach here (see `Status::offending_checks` on the Rust side) —
/// broken reads as a warning since it says nothing about the system under
/// test, just the check itself.
const CHECK_CHIP_COLOR: Record<string, "error" | "warning"> = {
	failed: "error",
	warning: "warning",
	broken: "warning",
};

/// Dedicated page for a single healthcheck: every live server whose
/// *current* status flags it, most urgent first. Doubles as an operator
/// TODO list for normalising those servers back to healthy, and as a way
/// to see who's sharing the same issue during a fleet-wide incident.
/// Linked from wherever a check name shows up — server detail, issue
/// rows, and the healthchecks settings catalog.
export default function HealthcheckAttention() {
	const { check } = useParams<{ check: string }>();
	usePageTitle(check ?? "Healthcheck");
	const tick = useReloadInterval(30_000, "canopy-data-changed");
	const result = useApi(
		"statuses",
		"check_attention",
		{ check: check ?? "" },
		[check, tick],
	);

	return (
		<Stack spacing={2}>
			<Box>
				<Typography variant="body2" color="text.secondary">
					<RouterLink to="/status">← Status</RouterLink>
				</Typography>
				<Stack direction="row" spacing={1} sx={{ alignItems: "center", mt: 0.5 }}>
					<Typography variant="h6" component="h2" sx={{ fontFamily: "monospace" }}>
						{check}
					</Typography>
					{result.status === "ok" && result.data.severity && (
						<SeverityChip severity={result.data.severity} />
					)}
				</Stack>
				<Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
					Servers whose latest status currently flags this check.{" "}
					<MuiLink
						component={RouterLink}
						to={`/settings/healthchecks/${encodeURIComponent(check ?? "")}`}
					>
						Configure severity / rules
					</MuiLink>
				</Typography>
			</Box>

			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : result.data.servers.length === 0 ? (
				<Alert severity="success">
					No servers currently flag <code>{check}</code>.
				</Alert>
			) : (
				<Paper variant="outlined">
					<TableContainer>
						<Table size="small">
							<TableHead>
								<TableRow>
									<TableCell>Server</TableCell>
									<TableCell>Result</TableCell>
									<TableCell>As of</TableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{result.data.servers.map((server) => (
									<AttentionRow key={server.server_id} server={server} />
								))}
							</TableBody>
						</Table>
					</TableContainer>
				</Paper>
			)}
		</Stack>
	);
}

function AttentionRow({ server }: { server: CheckAttentionServerData }) {
	return (
		<TableRow hover>
			<TableCell>
				<MuiLink component={RouterLink} to={`/servers/${server.server_id}`}>
					<ServerNameWithGroup
						groupName={server.group_name}
						serverName={server.server_name || "(unnamed)"}
					/>
				</MuiLink>
			</TableCell>
			<TableCell>
				<Chip
					label={server.result}
					size="small"
					variant="outlined"
					color={CHECK_CHIP_COLOR[server.result] ?? "warning"}
				/>
			</TableCell>
			<TableCell>
				<TimeAgo timestamp={server.status_created_at} />
			</TableCell>
		</TableRow>
	);
}
