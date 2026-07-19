import {
	Accordion,
	AccordionDetails,
	AccordionSummary,
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
import CheckStabilityPanel, {
	FleetStabilitySummary,
} from "../components/CheckStabilityPanel";
import Markdown from "../components/Markdown";
import CheckResultChip from "../components/CheckResultChip";
import ServerNameWithGroup from "../components/ServerNameWithGroup";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { type CheckDetailServerData, type CheckResult } from "../types";

const HEALTHY_RESULTS: readonly string[] = ["passed", "skipped"];

/// Detail page for a single healthcheck — one (source, check), since
/// that pair is the check's identity. Three sections: the operator
/// documentation, the check's fleet-wide stability rollup, and the
/// needs-attention list — every live server whose *current* state from
/// that source flags it, most urgent first, with the servers reporting
/// it healthy behind a toggle. The attention list doubles as an operator
/// TODO list for normalising those servers back to healthy, and as a way
/// to see who's sharing the same issue during a fleet-wide incident.
/// Linked from wherever a check name shows up — server detail, issue
/// rows, and the healthchecks settings catalog.
export default function CheckDetail() {
	const { source, check } = useParams<{ source: string; check: string }>();
	usePageTitle(check ?? "Healthcheck");
	const tick = useReloadInterval(30_000, "canopy-data-changed");
	const [showHealthy, setShowHealthy] = useState(false);
	const result = useApi(
		"statuses",
		"check_detail",
		{ source: source ?? "", check: check ?? "" },
		[source, check, tick],
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
					<Typography variant="body2" color="text.secondary">
						reported by {source}
					</Typography>
					{result.status === "ok" && result.data.ceiling && (
						<CheckResultChip result={result.data.ceiling as CheckResult} />
					)}
					{result.status === "ok" && result.data.escalates && (
						<Chip
							label="escalates"
							color="error"
							size="small"
							variant="outlined"
							title="An effective failure notifies immediately, bypassing the incident grace period"
						/>
					)}
				</Stack>
				<Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
					<MuiLink
						component={RouterLink}
						to={`/settings/healthchecks/${encodeURIComponent(check ?? "")}`}
					>
						Configure ceiling / rules / documentation
					</MuiLink>
				</Typography>
			</Box>

			{result.status === "ok" && result.data.documentation && (
				<Accordion variant="outlined" disableGutters>
					<AccordionSummary expandIcon={<ExpandMoreIcon />}>
						<Typography variant="subtitle2">About this check</Typography>
					</AccordionSummary>
					<AccordionDetails>
						<Markdown>{result.data.documentation}</Markdown>
					</AccordionDetails>
				</Accordion>
			)}

			{result.status === "ok" && (
				<FleetStability servers={result.data.servers} />
			)}

			<Box>
				<Typography variant="h6" component="h2">
					Needs attention
				</Typography>
				<Typography variant="body2" color="text.secondary">
					Servers whose latest report from this source currently flags this
					check, most urgent first.
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

/// The check's stability across the whole fleet: one heatmap over every
/// server's duty profile (healthy reporters included), so a shared
/// load-dependent pattern reads at a glance even when no single server
/// stands out. Hidden until at least one server has a record.
function FleetStability({ servers }: { servers: CheckDetailServerData[] }) {
	const records = servers.flatMap((s) => (s.stability ? [s.stability] : []));
	if (records.length === 0) return null;
	return (
		<Box>
			<Typography variant="h6" component="h2" sx={{ mb: 1 }}>
				Fleet stability
			</Typography>
			<FleetStabilitySummary records={records} />
		</Box>
	);
}

function ServersTable({
	check,
	servers,
	showHealthy,
}: {
	check: string;
	servers: CheckDetailServerData[];
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
							<TableCell>Stability</TableCell>
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
function AttentionRow({ server }: { server: CheckDetailServerData }) {
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
					<CheckResultChip
						result={server.result as CheckResult}
						variant="outlined"
					/>
				</TableCell>
				<TableCell>
					<StabilityCell stability={server.stability} />
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
					<TableCell colSpan={6} sx={{ py: 1 }}>
						<Stack spacing={2}>
							{extras.length > 0 ? (
								<CheckExtrasList extras={extras} />
							) : (
								<Typography variant="body2" color="text.secondary">
									No additional data reported for this check.
								</Typography>
							)}
							{server.stability && (
								<CheckStabilityPanel stability={server.stability} />
							)}
						</Stack>
					</TableCell>
				</TableRow>
			)}
		</>
	);
}

/// Compact flap summary for the table: recent state changes, or steady /
/// unknown. The expanded row carries the full record.
function StabilityCell({
	stability,
}: {
	stability: CheckDetailServerData["stability"];
}) {
	if (!stability) {
		return (
			<Typography variant="body2" color="text.secondary">
				no record
			</Typography>
		);
	}
	const { flips_24h, flips_7d } = stability.stats;
	if (flips_7d === 0) {
		return (
			<Typography variant="body2" color="text.secondary">
				steady
			</Typography>
		);
	}
	return (
		<Typography variant="body2">
			{flips_24h > 0 ? `${flips_24h} flips/24h` : `${flips_7d} flips/7d`}
		</Typography>
	);
}
