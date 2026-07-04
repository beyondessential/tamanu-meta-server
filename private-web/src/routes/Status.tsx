import {
	Alert,
	Box,
	Card,
	CardContent,
	Chip,
	LinearProgress,
	Link as MuiLink,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import PersonIcon from "@mui/icons-material/Person";
import { Link as RouterLink } from "react-router-dom";
import StatusDot from "../components/StatusDot";
import VersionIndicator from "../components/VersionIndicator";
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
	compareServersByRankThenKind,
} from "../types";

/// Held vs loud: a group has an open incident, but the Slack notice is
/// still inside the per-group cooldown window (held) or has already fired
/// or been cancelled (loud). Drives the warning-vs-error colouring on the
/// Status page and elsewhere.
export type IncidentLoudness = "held" | "loud";

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
			? incidents.data.map((i) => [
					i.server_group_id,
					i.notification_held_until &&
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
				<Typography variant="body1">
					{releases.length} release branches in active use:{" "}
					{releases
						.map(([major, minor]) => `${major}.${minor}`)
						.join(", ")}{" "}
					({versions.length} versions: {bracket.min} — {bracket.max})
				</Typography>
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

	const sections = SERVER_RANK_ORDER.flatMap((rank) => {
		const ids = grouped.data[rank];
		if (!ids || ids.length === 0) return [];
		return [{ rank, ids }];
	});

	if (sections.length === 0) {
		return (
			<Alert severity="info">
				No server groups configured. Create one via the Servers page.
			</Alert>
		);
	}

	return (
		<Stack spacing={3}>
			{sections.map(({ rank, ids }) => (
				<Box key={rank}>
					<Typography
						variant="h5"
						component="h2"
						sx={{ textTransform: "capitalize", mb: 1 }}
					>
						{rank}
					</Typography>
					<Box
						sx={{
							display: "grid",
							gridTemplateColumns: "repeat(auto-fill, minmax(18em, 1fr))",
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
				</Box>
			))}
		</Stack>
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
	// might still self-resolve". Loud incidents stay full red.
	const borderColor =
		openIncident === "loud"
			? "error.main"
			: openIncident === "held"
				? "warning.main"
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
				<CardContent sx={{ p: 1.5, "&:last-child": { pb: 1.5 } }}>
					{result.status === "loading" || result.status === "idle" ? (
						<Typography variant="body2" color="text.secondary">
							Thinking…
						</Typography>
					) : result.status === "error" ? (
						<Alert severity="error">{result.error.message}</Alert>
					) : (
						<GroupCard
							group={result.data}
							operators={operators}
							openIncident={openIncident}
						/>
					)}
				</CardContent>
			</Card>
		</MuiLink>
	);
}

function GroupCard({
	group,
	operators,
	openIncident,
}: {
	group: ServerGroupCard;
	operators: AggregatedOperator[];
	openIncident: IncidentLoudness | null;
}) {
	return (
		<Stack spacing={1}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "baseline", justifyContent: "space-between" }}
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
						distance={group.version_distance}
						addLink={false}
					/>
				</Box>
			</Stack>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<RankedDotStrip members={group.members} />
				<Stack direction="row" spacing={0.5} sx={{ alignItems: "center" }}>
					{operators.length > 0 && (
						<OperatorCountChip operators={operators} />
					)}
					{openIncident === "loud" && (
						<Chip label="incident" color="error" size="small" />
					)}
					{openIncident === "held" && (
						<Tooltip title="Open incident; Slack notice is still inside the per-group cooldown window">
							<Chip label="incident (held)" color="warning" size="small" />
						</Tooltip>
					)}
				</Stack>
			</Stack>
		</Stack>
	);
}

/// Compact "people are in here" marker on a group card: person icon +
/// distinct operator count, tooltip naming who's on which server.
function OperatorCountChip({
	operators,
}: {
	operators: AggregatedOperator[];
}) {
	return (
		<Tooltip
			title={
				<Box>
					{operators.map(({ op, servers }) => (
						<Box key={op.login}>
							{op.login} · {servers.join(", ")}
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

/// Strip of StatusDots for a group's members, sorted by rank then kind
/// (centrals first within a rank). A hollow right-pointing triangle
/// separates adjacent ranks, so operators can see at a glance how the
/// group breaks down without naming each dot.
// Every child of the strip — dot or rank separator — sits in an identical
// fixed-size cell, so wrapped rows always align to the same column grid no
// matter where the line breaks fall. Spacing comes from the container's
// `gap`, not per-dot margins (StatusDot's inline right-margin is neutralised).
const dotCellSx = {
	display: "inline-flex",
	width: "1em",
	height: "1em",
	alignItems: "center",
	justifyContent: "center",
	flex: "none",
	"& > span": { marginRight: 0 },
} as const;

export function RankedDotStrip({ members }: { members: FacilityServerStatus[] }) {
	const sorted = [...members].sort(compareServersByRankThenKind);
	const cells: React.ReactNode[] = [];
	let prevRank: string | null = null;
	for (const m of sorted) {
		const rank = m.rank ?? "_unranked";
		if (prevRank != null && rank !== prevRank) {
			cells.push(
				<Box
					key={`sep-${rank}`}
					component="span"
					aria-hidden
					sx={{ ...dotCellSx, color: "text.primary" }}
				>
					{/* Hollow play-button triangle, dot-sized; MUI's
					    PlayArrowOutlined renders too small in a 1em cell and
					    has sharp corners, hence the inline SVG. */}
					<svg
						width="1em"
						height="1em"
						viewBox="0 0 16 16"
						fill="none"
						stroke="currentColor"
						strokeWidth={2}
						strokeLinejoin="round"
						strokeLinecap="round"
					>
						<path d="M4.5 3.2 12.8 8 4.5 12.8Z" />
					</svg>
				</Box>,
			);
		}
		prevRank = rank;
		cells.push(
			<Tooltip
				key={m.id}
				title={`${m.name || "(unnamed)"}${
					m.rank ? ` · ${m.rank}` : ""
				} · ${m.kind}`}
			>
				<Box component="span" sx={dotCellSx}>
					<StatusDot
						up={m.up}
						health={m.health}
						title={`${m.name}: ${m.up}${
							m.health !== "healthy" ? ` (${m.health})` : ""
						}`}
					/>
				</Box>
			</Tooltip>,
		);
	}
	return (
		<Stack
			direction="row"
			spacing={0}
			sx={{ flexWrap: "wrap", alignItems: "center", gap: "0.5em" }}
		>
			{cells}
		</Stack>
	);
}
