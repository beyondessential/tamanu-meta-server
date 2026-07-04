import {
	Alert,
	Box,
	Chip,
	FormControlLabel,
	IconButton,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Stack,
	Switch,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	Typography,
} from "@mui/material";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import { useState } from "react";
import { Link as RouterLink, useParams } from "react-router-dom";
import { useApi } from "../api";
import CheckExtrasList, { checkEntryExtras } from "../components/CheckExtras";
import ServerNameWithGroup from "../components/ServerNameWithGroup";
import SeverityChip from "../components/SeverityChip";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { type CheckAttentionServerData, type CheckResult } from "../types";

/// MUI chip colour per check result. Broken reads as a warning since it
/// says nothing about the system under test, just the check itself;
/// passed/skipped (visible behind the "show healthy" toggle) read calm.
const CHECK_CHIP_COLOR: Record<
	CheckResult,
	"error" | "warning" | "success" | "default"
> = {
	failed: "error",
	warning: "warning",
	broken: "warning",
	passed: "success",
	skipped: "default",
};

const HEALTHY_RESULTS: readonly string[] = ["passed", "skipped"];

/// Dedicated page for a single healthcheck: every live server whose
/// *current* status flags it, most urgent first, with the servers
/// reporting it healthy behind a toggle. Doubles as an operator TODO
/// list for normalising those servers back to healthy, and as a way to
/// see who's sharing the same issue during a fleet-wide incident.
/// Linked from wherever a check name shows up — server detail, issue
/// rows, and the healthchecks settings catalog.
export default function HealthcheckAttention() {
	const { check } = useParams<{ check: string }>();
	usePageTitle(check ?? "Healthcheck");
	const tick = useReloadInterval(30_000, "canopy-data-changed");
	const [showHealthy, setShowHealthy] = useState(false);
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

			<FormControlLabel
				control={
					<Switch
						size="small"
						checked={showHealthy}
						onChange={(e) => setShowHealthy(e.target.checked)}
					/>
				}
				label="Show healthy servers for this check"
			/>

			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : (
				<ServersTable
					check={check ?? ""}
					servers={result.data.servers}
					showHealthy={showHealthy}
				/>
			)}
		</Stack>
	);
}

function ServersTable({
	check,
	servers,
	showHealthy,
}: {
	check: string;
	servers: CheckAttentionServerData[];
	showHealthy: boolean;
}) {
	const visible = showHealthy
		? servers
		: servers.filter((s) => !HEALTHY_RESULTS.includes(s.result));

	if (visible.length === 0) {
		const healthyCount = servers.length;
		return (
			<Alert severity="success">
				No servers currently flag <code>{check}</code>.
				{healthyCount > 0 &&
					` ${healthyCount} ${
						healthyCount === 1 ? "server reports" : "servers report"
					} it healthy — use the toggle above to see them.`}
			</Alert>
		);
	}

	return (
		<Paper variant="outlined">
			<TableContainer>
				<Table size="small">
					<TableHead>
						<TableRow>
							<TableCell width={40} />
							<TableCell>Server</TableCell>
							<TableCell>Result</TableCell>
							<TableCell>Failing since</TableCell>
							<TableCell>As of</TableCell>
						</TableRow>
					</TableHead>
					<TableBody>
						{visible.map((server) => (
							<AttentionRow key={server.server_id} server={server} />
						))}
					</TableBody>
				</Table>
			</TableContainer>
		</Paper>
	);
}

/// One server row, expandable to the check's full `health[]` entry data —
/// the same key/value rendering the server detail checks table uses.
function AttentionRow({ server }: { server: CheckAttentionServerData }) {
	const [expanded, setExpanded] = useState(false);
	const entry =
		typeof server.data === "object" &&
		server.data !== null &&
		!Array.isArray(server.data)
			? (server.data as Record<string, unknown>)
			: {};
	const extras = checkEntryExtras(entry);
	return (
		<>
			<TableRow hover>
				<TableCell>
					<IconButton
						aria-label={expanded ? "Collapse" : "Expand"}
						size="small"
						onClick={() => setExpanded((v) => !v)}
					>
						{expanded ? (
							<ExpandLessIcon fontSize="small" />
						) : (
							<ExpandMoreIcon fontSize="small" />
						)}
					</IconButton>
				</TableCell>
				<TableCell>
					<ServerNameWithGroup
						groupName={server.group_name}
						groupId={server.group_id}
						serverName={server.server_name || "(unnamed)"}
						serverId={server.server_id}
					/>
				</TableCell>
				<TableCell>
					<Chip
						label={server.result}
						size="small"
						variant="outlined"
						color={CHECK_CHIP_COLOR[server.result as CheckResult] ?? "warning"}
					/>
				</TableCell>
				<TableCell>
					{server.failing_since ? (
						<TimeAgo timestamp={server.failing_since} />
					) : (
						<Typography variant="body2" color="text.secondary">
							—
						</Typography>
					)}
				</TableCell>
				<TableCell>
					<TimeAgo timestamp={server.status_created_at} />
				</TableCell>
			</TableRow>
			{expanded && (
				<TableRow>
					<TableCell colSpan={5} sx={{ py: 1 }}>
						{extras.length > 0 ? (
							<CheckExtrasList extras={extras} />
						) : (
							<Typography variant="body2" color="text.secondary">
								No additional data reported for this check.
							</Typography>
						)}
					</TableCell>
				</TableRow>
			)}
		</>
	);
}
