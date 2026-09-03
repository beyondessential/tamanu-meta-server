import {
	Alert,
	Box,
	Button,
	Card,
	CardContent,
	Chip,
	LinearProgress,
	Link as MuiLink,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import BarChartIcon from "@mui/icons-material/BarChart";
import PersonIcon from "@mui/icons-material/Person";
import { useMemo } from "react";
import { Link as RouterLink } from "react-router-dom";
import MachineEnclosure from "../components/MachineEnclosure";
import StatusDot from "../components/StatusDot";
import VersionIndicator from "../components/VersionIndicator";
import { useVersionTrackingAcross } from "../hooks/useApplicationTypes";
import {
	HealthLegend,
	OperatorLegend,
	StatusLegend,
	VersionLegend,
} from "../components/Legends";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import {
	type AggregatedOperator,
	type FacilityServerStatus,
	type ServerGroupCard,
	SERVER_RANK_ORDER,
	aggregateOperators,
	compareServersByRankThenType,
	groupServersByRank,
	isIncidentLingering,
} from "../types";

/// How an open incident should read at a glance:
/// - "loud": failing, and the Slack notice has fired (or been given up on);
/// - "held": failing, but the notice is still inside the per-group cooldown
///   window — nobody has been paged yet;
/// - "lingering": every failure has recovered, and the incident is waiting
///   out the group's linger window in case one comes back.
/// Drives the error/warning/info colouring on the Status page and elsewhere.
export type IncidentLoudness = "held" | "loud" | "lingering";

export default function Status() {
	usePageTitle("Status");
	const tick = useReloadInterval(60_000, "canopy-reload-status");
	const incidents = useApi("incidents", "list_active", {}, [tick]);
	// `held` only counts while the deliver-after deadline is still ahead:
	// once it slips into the past the worker has shipped (or is about to)
	// and this page is just behind on the data — fall through to "loud"
	// rather than asserting a held state we can't verify. The 60s tick
	// re-evaluates this naturally.
	const now = Date.now();
	const openIncidentGroups = new Map<string, IncidentLoudness>(
		incidents.status === "ok"
			? incidents.data
					// Canopy-wide incidents (no group) surface via the
					// self-alerts banner, not the group cards.
					.filter((i) => i.server_group_id != null)
					.map((i): [string, IncidentLoudness] => [
						i.server_group_id as string,
						// Lingering wins: the group is currently green, so
						// painting it red/yellow would overstate the trouble.
						isIncidentLingering(i)
							? "lingering"
							: i.notification_held_until &&
									Date.parse(i.notification_held_until) > now
								? "held"
								: "loud",
					])
			: [],
	);
	return (
		<Stack spacing={3}>
			<ReleaseSummary tick={tick} />
			<GroupCards tick={tick} openIncidentGroups={openIncidentGroups} />
			<Box>
				<VersionLegend />
				<Box sx={{ mt: 1 }}>
					<StatusLegend />
				</Box>
				<Box sx={{ mt: 1 }}>
					<HealthLegend />
				</Box>
				<Box sx={{ mt: 1 }}>
					<OperatorLegend />
				</Box>
			</Box>
		</Stack>
	);
}

function ReleaseSummary({ tick }: { tick: number }) {
	const result = useApi("statuses", "summary", {}, [tick]);
	if (result.status === "loading" || result.status === "idle") {
		return <LinearProgress />;
	}
	if (result.status === "error") {
		return <Alert severity="error">{result.error.message}</Alert>;
	}
	const { releases, versions, bracket } = result.data;
	return (
		<Card variant="outlined">
			<CardContent>
				<Stack
					direction="row"
					spacing={2}
					sx={{
						alignItems: "center",
						justifyContent: "space-between",
						flexWrap: "wrap",
					}}
					useFlexGap
				>
					<Typography variant="body1">
						{releases.length} release{" "}
						{releases.length === 1 ? "branch" : "branches"} in active use:{" "}
						{releases
							.map(([major, minor]) => `${major}.${minor}`)
							.join(", ")}{" "}
						({versions.length}{" "}
						{versions.length === 1 ? "version" : "versions"}: {bracket.min} —{" "}
						{bracket.max})
					</Typography>
					{/* The version spread is one of several figures the fleet
					    reports; this card answers "which branches", the figures
					    page answers "which servers, and what else are they on". */}
					<Button
						component={RouterLink}
						to="/servers/figures"
						variant="outlined"
						size="small"
						startIcon={<BarChartIcon />}
						sx={{ flexShrink: 0 }}
					>
						Fleet figures
					</Button>
				</Stack>
			</CardContent>
		</Card>
	);
}

function GroupCards({
	tick,
	openIncidentGroups,
}: {
	tick: number;
	openIncidentGroups: Map<string, IncidentLoudness>;
}) {
	const grouped = useApi(
		"statuses",
		"server_grouped_ids",
		{},
		[tick],
	);

	if (grouped.status === "loading" || grouped.status === "idle") {
		return <LinearProgress />;
	}
	if (grouped.status === "error") {
		return <Alert severity="error">{grouped.error.message}</Alert>;
	}

	// One grid, not a section per rank. Each card carries its own ranks in its
	// dot strip, so a heading above it names again what the card already says,
	// and the section breaks cost a row of reflow apiece. Rank still orders the
	// cards — production leads — it just no longer divides them.
	// spec: CHK#presentation
	const ids = SERVER_RANK_ORDER.flatMap((rank) => grouped.data[rank] ?? []);

	if (ids.length === 0) {
		return (
			<Alert severity="info">
				No server groups configured. Create one via the Servers page.
			</Alert>
		);
	}

	return (
		<Box
			sx={{
				display: "grid",
				// The banded card reads at a narrower measure than the old one
				// did: its name band ellipsises and its dot strip wraps, so the
				// column can be sized to fit more of the fleet on a screen.
				gridTemplateColumns: "repeat(auto-fill, minmax(16em, 1fr))",
				gap: 1.5,
			}}
		>
			{ids.map((id) => (
				<GroupCardLoader
					key={id}
					groupId={id}
					tick={tick}
					openIncident={openIncidentGroups.get(id) ?? null}
				/>
			))}
		</Box>
	);
}

function GroupCardLoader({
	groupId,
	tick,
	openIncident,
}: {
	groupId: string;
	tick: number;
	openIncident: IncidentLoudness | null;
}) {
	const result = useApi(
		"statuses",
		"group_details",
		{ server_group_id: groupId },
		[groupId, tick],
	);

	// Held incidents tone the border down to warning so an operator can see
	// at a glance "yes there's a thing, but Slack hasn't been told yet — it
	// might still self-resolve". Lingering incidents tone down further to
	// info: everything has recovered and the incident is just waiting out
	// its linger window. Loud incidents stay full red.
	const borderColor =
		openIncident === "loud"
			? "error.main"
			: openIncident === "held"
				? "warning.main"
				: openIncident === "lingering"
					? "info.main"
					: undefined;

	// Active operator presence anywhere in the group tints the card, so
	// "someone is already on it" reads at a glance — especially useful
	// next to an incident border. Hover steps the tint up a notch instead
	// of disappearing into the base shade.
	const operators =
		result.status === "ok" ? aggregateOperators(result.data.members) : [];
	const occupied = operators.length > 0;

	return (
		<MuiLink
			component={RouterLink}
			to={`/groups/${groupId}`}
			underline="none"
			color="inherit"
		>
			<Card
				variant="outlined"
				sx={{
					// The bands run edge to edge, so the card clips them to its
					// own rounded corners.
					overflow: "hidden",
					transition: "background-color 150ms",
					bgcolor: occupied ? "action.hover" : undefined,
					"&:hover": {
						bgcolor: occupied ? "action.selected" : "action.hover",
					},
					...(borderColor && {
						borderColor,
						borderWidth: 2,
					}),
				}}
			>
				{result.status === "loading" || result.status === "idle" ? (
					<CardContent sx={{ p: 1.5, "&:last-child": { pb: 1.5 } }}>
						<Typography variant="body2" color="text.secondary">
							Thinking…
						</Typography>
					</CardContent>
				) : result.status === "error" ? (
					<CardContent sx={{ p: 1.5, "&:last-child": { pb: 1.5 } }}>
						<Alert severity="error">{result.error.message}</Alert>
					</CardContent>
				) : (
					<GroupCard
						group={result.data}
						operators={operators}
						openIncident={openIncident}
					/>
				)}
			</Card>
		</MuiLink>
	);
}

/// A group card, in three bands: what it is, what is in it, and what is
/// happening to it.
///
/// The status band is omitted when there is neither an operator nor an
/// incident, so a quiet card is two bands and the eye is drawn to the ones
/// that have a third.
/// spec: CHK#presentation
function GroupCard({
	group,
	operators,
	openIncident,
}: {
	group: ServerGroupCard;
	operators: AggregatedOperator[];
	openIncident: IncidentLoudness | null;
}) {
	// The headline version comes from whichever member speaks for the group, so
	// how to present it follows from the types its members actually have: a
	// group with no versioned member shows no version rather than an "unknown".
	// spec: APP#versions
	const tracking = useVersionTrackingAcross(
		useMemo(() => group.members.map((m) => m.type), [group.members]),
	);
	const hasStatusBand = operators.length > 0 || openIncident !== null;
	return (
		<Box>
			<Box
				sx={{
					display: "flex",
					alignItems: "baseline",
					justifyContent: "space-between",
					gap: 1,
					px: "10px",
					py: "8px",
					borderBottom: 1,
					borderColor: "divider",
				}}
			>
				<Typography
					variant="subtitle1"
					component="h3"
					sx={{
						overflow: "hidden",
						textOverflow: "ellipsis",
						whiteSpace: "nowrap",
						minWidth: 0,
					}}
				>
					{group.name}
				</Typography>
				<Box sx={{ flexShrink: 0 }}>
					<VersionIndicator
						version={group.version}
						tracking={tracking}
						distance={group.version_distance}
						addLink={false}
					/>
				</Box>
			</Box>

			<RankedDotStrip members={group.members} />

			{hasStatusBand && (
				<Box
					sx={{
						display: "flex",
						alignItems: "stretch",
						borderTop: 1,
						borderColor: "divider",
						fontSize: "0.6875rem",
					}}
				>
					{operators.length > 0 && (
						<Box
							sx={{
								display: "flex",
								alignItems: "center",
								gap: 0.5,
								px: 1,
								py: 0.5,
								bgcolor: "action.hover",
								color: "text.secondary",
								// Only when something sits beside it; the segment
								// squares off the card's bottom edge otherwise.
								...(openIncident !== null
									? { borderRight: 1, borderColor: "divider" }
									: { flex: 1 }),
							}}
						>
							<OperatorCountChip operators={operators} />
						</Box>
					)}
					{openIncident !== null && (
						<Box
							sx={{
								flex: 1,
								display: "flex",
								alignItems: "center",
								justifyContent: "flex-end",
								px: 1,
								py: 0.5,
								bgcolor: "action.hover",
								color: "text.secondary",
							}}
						>
							<IncidentMark loudness={openIncident} />
						</Box>
					)}
				</Box>
			)}
		</Box>
	);
}

/// What an open incident on this group reads as, which is not the same as how
/// loud it is: a held one is not yet in Slack, and a recovering one has had its
/// failures clear and is waiting out the linger window.
function IncidentMark({ loudness }: { loudness: IncidentLoudness }) {
	if (loudness === "loud") {
		return <Chip label="incident" color="error" size="small" />;
	}
	if (loudness === "held") {
		return (
			<Tooltip title="Open incident; Slack notice is still inside the per-group cooldown window">
				<Chip label="incident (held)" color="warning" size="small" />
			</Tooltip>
		);
	}
	return (
		<Tooltip title="Open incident whose failures have all recovered; it closes if they stay quiet through the linger window">
			<Chip label="incident (recovering)" color="info" size="small" />
		</Tooltip>
	);
}

/// Compact "people are in here" marker on a group card: person icon +
/// distinct operator count, tooltip naming who's on which box.
function OperatorCountChip({
	operators,
}: {
	operators: AggregatedOperator[];
}) {
	return (
		<Tooltip
			title={
				<Box>
					{operators.map(({ op, machines }) => (
						<Box key={op.login}>
							{op.login} · {machines.join(", ")}
						</Box>
					))}
				</Box>
			}
		>
			<Chip
				icon={<PersonIcon />}
				label={operators.length}
				size="small"
				variant="outlined"
			/>
		</Tooltip>
	);
}

/// A group's members as rank rows of machine enclosures.
///
/// Rank is the outer break, highest first, since that is how an operator reads
/// a group. Within a rank every machine is a pill holding the dots for the
/// applications on it: a one-application box is one dot in a pill, so the
/// presence of an enclosure never means anything on its own — only its
/// contents do. Two dots in one pill is the case the machine grain exists for.
/// spec: FLT
// Every dot sits in an identical fixed-size cell, so wrapped rows align to the
// same column grid wherever the line breaks fall. Spacing comes from the
// container's `gap`, not per-dot margins (StatusDot's inline right-margin is
// neutralised).
const dotCellSx = {
	display: "inline-flex",
	width: "1em",
	height: "1em",
	alignItems: "center",
	justifyContent: "center",
	flex: "none",
	"& > span": { marginRight: 0 },
} as const;

/// The machines of one group, each with the applications on it, bucketed by
/// the rank of the highest-ranked application it carries.
function machineRows(members: FacilityServerStatus[]) {
	const byMachine = new Map<string, FacilityServerStatus[]>();
	for (const m of members) {
		const on = byMachine.get(m.machine_id);
		if (on) on.push(m);
		else byMachine.set(m.machine_id, [m]);
	}
	const boxes = [...byMachine.values()].map((on) => {
		const sorted = [...on].sort(compareServersByRankThenType);
		return { applications: sorted, lead: sorted[0]! };
	});
	return groupServersByRank(
		boxes.map((b) => ({
			...b,
			rank: b.lead.rank,
			type: b.lead.type,
			name: b.lead.machine_name ?? b.lead.name,
		})),
	);
}

export function RankedDotStrip({ members }: { members: FacilityServerStatus[] }) {
	const rows = machineRows(members);
	return (
		<Stack data-testid="dot-strip" spacing={0.5} sx={{ minWidth: 0 }}>
			{rows.map(([rank, boxes], index) => (
				<Box
					key={rank ?? "_unranked"}
					data-testid="rank-row"
					data-rank={rank ?? "unranked"}
					sx={{
						position: "relative",
						display: "flex",
						flexWrap: "wrap",
						alignItems: "center",
						gap: "0.4em",
						px: "10px",
						py: "7px",
						// A rule lighter than the card's own borders, so the rank
						// break reads as subordinate to the card structure.
						...(index > 0
							? { borderTop: 1, borderColor: "divider" }
							: {}),
						// The rank spelled out behind its own row, faint enough to
						// read only when looked for. It replaces the triangle that
						// used to mark the break without naming it, and needs no
						// space of its own.
						"&::after": {
							content: "attr(data-rank)",
							position: "absolute",
							right: "8px",
							top: "50%",
							transform: "translateY(-50%)",
							fontSize: "0.9375rem",
							fontWeight: 500,
							letterSpacing: "0.06em",
							textTransform: "uppercase",
							color: "rgba(0, 0, 0, 0.09)",
							pointerEvents: "none",
							zIndex: 0,
						},
					}}
				>
					{boxes.map((box) => (
						<MachineEnclosure
							key={box.lead.machine_id}
							up={box.lead.machine_up}
							health={box.lead.machine_health}
							name={box.lead.machine_name}
							maintained={box.lead.machine_maintained}
						>
							{box.applications.map((m) => (
								<Box key={m.id} component="span" sx={dotCellSx}>
									<StatusDot
										up={m.up}
										health={m.health}
										monitored={m.is_monitored}
										title={`${m.name}${
											m.rank ? ` · ${m.rank}` : ""
										} · ${m.type}`}
									/>
								</Box>
							))}
						</MachineEnclosure>
					))}
				</Box>
			))}
		</Stack>
	);
}
