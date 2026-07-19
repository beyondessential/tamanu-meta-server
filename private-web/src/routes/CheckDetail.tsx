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
import { Fragment, useState } from "react";
import { Link as RouterLink, useParams } from "react-router-dom";
import { useApi } from "../api";
import CheckExtrasList, { checkEntryExtras } from "../components/CheckExtras";
import CheckStabilityPanel, {
	FleetStabilitySummary,
} from "../components/CheckStabilityPanel";
import Markdown from "../components/Markdown";
import CheckResultChip from "../components/CheckResultChip";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import {
	SERVER_RANK_ORDER,
	compareServersByRankThenKind,
	type CheckDetailData,
	type CheckDetailGroupData,
	type CheckDetailServerData,
	type CheckResult,
	type ServerRank,
} from "../types";

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

			{result.status === "ok" && <FleetStability data={result.data} />}

			<Box>
				<Typography variant="h6" component="h2">
					Needs attention
				</Typography>
				<Typography variant="body2" color="text.secondary">
					Everything whose current state from this source flags this check —
					servers, whole groups, and canopy itself — by rank and group.
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
				<AttentionList
					check={check ?? ""}
					data={result.data}
					showHealthy={showHealthy}
				/>
			)}
		</Stack>
	);
}

/// The check's stability across the whole fleet: one heatmap over every
/// target's duty profile (healthy reporters included, group and canopy
/// states too), so a shared load-dependent pattern reads at a glance
/// even when no single target stands out. Hidden until at least one
/// target has a record.
function FleetStability({ data }: { data: CheckDetailData }) {
	const serverRecords = data.servers.flatMap((s) =>
		s.stability ? [s.stability] : [],
	);
	const groupRecords = data.groups.flatMap((g) =>
		g.stability ? [g.stability] : [],
	);
	const canopyRecord = data.canopy?.stability ?? null;
	const records = [
		...serverRecords,
		...groupRecords,
		...(canopyRecord ? [canopyRecord] : []),
	];
	if (records.length === 0) return null;
	const subject = [
		serverRecords.length > 0 &&
			`${serverRecords.length} server${serverRecords.length === 1 ? "" : "s"}`,
		groupRecords.length > 0 &&
			`${groupRecords.length} group${groupRecords.length === 1 ? "" : "s"}`,
		canopyRecord && "canopy",
	]
		.filter(Boolean)
		.join(", ");
	return (
		<Box>
			<Typography variant="h6" component="h2" sx={{ mb: 1 }}>
				Fleet stability
			</Typography>
			<FleetStabilitySummary records={records} subject={subject} />
		</Box>
	);
}

type RankKey = ServerRank | null;

/// A row's state fields, shared by server, group, and canopy rows.
type StateFields = Pick<
	CheckDetailServerData,
	"result" | "data" | "failing_since" | "status_created_at" | "stability"
>;

/// One group's slice of a rank bucket: the group's own state for this
/// check (if any) followed by its member servers.
type Section = {
	key: string;
	groupId: string | null;
	groupName: string | null;
	groupState: CheckDetailGroupData | null;
	servers: CheckDetailServerData[];
};

/// The needs-attention list, in the standard list shape used across the
/// UI: rank buckets in display order (unranked last, without a heading),
/// groups alphabetical within a bucket — each with its own group-scoped
/// state first, then its member servers by kind then name — ungrouped
/// servers last, and canopy's own state as a trailing section.
function AttentionList({
	check,
	data,
	showHealthy,
}: {
	check: string;
	data: CheckDetailData;
	showHealthy: boolean;
}) {
	const healthy = (result: string) => HEALTHY_RESULTS.includes(result);
	const servers = showHealthy
		? data.servers
		: data.servers.filter((s) => !healthy(s.result));
	const groups = showHealthy
		? data.groups
		: data.groups.filter((g) => !healthy(g.result));
	const canopy =
		data.canopy && (showHealthy || !healthy(data.canopy.result))
			? data.canopy
			: null;

	if (servers.length === 0 && groups.length === 0 && !canopy) {
		const healthyCount =
			data.servers.length + data.groups.length + (data.canopy ? 1 : 0);
		return (
			<Alert severity="success">
				Nothing currently flags <code>{check}</code>.
				{healthyCount > 0 &&
					` ${healthyCount} ${
						healthyCount === 1 ? "reporter shows" : "reporters show"
					} it healthy — use the toggle above to see them.`}
			</Alert>
		);
	}

	const buckets = new Map<RankKey, Map<string, Section>>();
	const sectionFor = (
		rank: RankKey,
		groupId: string | null,
		groupName: string | null,
	): Section => {
		let bucket = buckets.get(rank);
		if (!bucket) {
			bucket = new Map();
			buckets.set(rank, bucket);
		}
		const key = groupId ?? "__ungrouped";
		let section = bucket.get(key);
		if (!section) {
			section = { key, groupId, groupName, groupState: null, servers: [] };
			bucket.set(key, section);
		}
		return section;
	};
	for (const g of groups) {
		sectionFor(g.rank, g.group_id, g.group_name).groupState = g;
	}
	for (const s of servers) {
		sectionFor(s.rank, s.group_id, s.group_name).servers.push(s);
	}

	const rankOrder: RankKey[] = [...SERVER_RANK_ORDER, null];
	return (
		<Stack spacing={2}>
			{rankOrder.map((rank) => {
				const bucket = buckets.get(rank);
				if (!bucket) return null;
				const sections = [...bucket.values()].sort((a, b) => {
					// Ungrouped servers trail the named groups.
					if ((a.groupId == null) !== (b.groupId == null)) {
						return a.groupId == null ? 1 : -1;
					}
					return (a.groupName ?? "").localeCompare(b.groupName ?? "");
				});
				return (
					<Box key={rank ?? "_unranked"}>
						{rank && (
							<Typography
								variant="overline"
								color="text.secondary"
								sx={{ display: "block", mb: 0.5 }}
							>
								{rank}
							</Typography>
						)}
						<StateTable>
							{sections.map((section) => (
								<Fragment key={section.key}>
									<TableRow>
										<TableCell
											colSpan={6}
											sx={{ bgcolor: "action.hover", py: 0.5 }}
										>
											{section.groupId ? (
												<MuiLink
													component={RouterLink}
													to={`/groups/${section.groupId}`}
													underline="hover"
													variant="subtitle2"
												>
													{section.groupName || "(unnamed group)"}
												</MuiLink>
											) : (
												<Typography
													variant="subtitle2"
													color="text.secondary"
												>
													Ungrouped
												</Typography>
											)}
										</TableCell>
									</TableRow>
									{section.groupState && (
										<StateRow
											label={
												<Typography
													variant="body2"
													sx={{ fontStyle: "italic" }}
												>
													whole group
												</Typography>
											}
											state={section.groupState}
										/>
									)}
									{[...section.servers]
										.sort(compareServersByRankThenKind_)
										.map((server) => (
											<StateRow
												key={server.server_id}
												label={
													<MuiLink
														component={RouterLink}
														to={`/servers/${server.server_id}`}
														underline="hover"
													>
														{server.server_name || "(unnamed)"}
													</MuiLink>
												}
												state={server}
											/>
										))}
								</Fragment>
							))}
						</StateTable>
					</Box>
				);
			})}
			{canopy && (
				<Box>
					<Typography
						variant="overline"
						color="text.secondary"
						sx={{ display: "block", mb: 0.5 }}
					>
						canopy
					</Typography>
					<StateTable>
						<StateRow
							label={
								<Typography variant="body2" sx={{ fontStyle: "italic" }}>
									Canopy (self-monitoring)
								</Typography>
							}
							state={canopy}
						/>
					</StateTable>
				</Box>
			)}
		</Stack>
	);
}

/// The servers within a section share a rank bucket; ordering reduces to
/// the standard kind-then-name comparison.
function compareServersByRankThenKind_(
	a: CheckDetailServerData,
	b: CheckDetailServerData,
): number {
	return compareServersByRankThenKind(
		{ rank: a.rank, kind: a.kind, name: a.server_name },
		{ rank: b.rank, kind: b.kind, name: b.server_name },
	);
}

function StateTable({ children }: { children: React.ReactNode }) {
	return (
		<Paper variant="outlined">
			<TableContainer>
				<Table size="small">
					<TableHead>
						<TableRow>
							<TableCell width={40} />
							<TableCell>Target</TableCell>
							<TableCell>Result</TableCell>
							<TableCell>Stability</TableCell>
							<TableCell>Failing since</TableCell>
							<TableCell>As of</TableCell>
						</TableRow>
					</TableHead>
					<TableBody>{children}</TableBody>
				</Table>
			</TableContainer>
		</Paper>
	);
}

/// One state row — a server, a whole group, or canopy itself — expandable
/// to the check's full data (the same key/value rendering the server
/// detail checks table uses) and the state's stability record.
function StateRow({
	label,
	state,
}: {
	label: React.ReactNode;
	state: StateFields;
}) {
	const [expanded, setExpanded] = useState(false);
	const entry =
		typeof state.data === "object" &&
		state.data !== null &&
		!Array.isArray(state.data)
			? (state.data as Record<string, unknown>)
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
				<TableCell>{label}</TableCell>
				<TableCell>
					<CheckResultChip
						result={state.result as CheckResult}
						variant="outlined"
					/>
				</TableCell>
				<TableCell>
					<StabilityCell stability={state.stability} />
				</TableCell>
				<TableCell>
					{state.failing_since ? (
						<TimeAgo timestamp={state.failing_since} />
					) : (
						<Typography variant="body2" color="text.secondary">
							—
						</Typography>
					)}
				</TableCell>
				<TableCell>
					<TimeAgo timestamp={state.status_created_at} />
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
							{state.stability && (
								<CheckStabilityPanel stability={state.stability} />
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
