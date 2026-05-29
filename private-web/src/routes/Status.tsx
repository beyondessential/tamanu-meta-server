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
import { Link as RouterLink } from "react-router-dom";
import StatusDot from "../components/StatusDot";
import VersionIndicator from "../components/VersionIndicator";
import { HealthLegend, StatusLegend, VersionLegend } from "../components/Legends";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import {
	type FacilityServerStatus,
	type ServerGroupCard,
	SERVER_RANK_ORDER,
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
					"&:hover": { bgcolor: "action.hover" },
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
	openIncident,
}: {
	group: ServerGroupCard;
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
	);
}

/// Strip of StatusDots for a group's members, sorted by rank then kind
/// (centrals first within a rank). A thin grey vertical bar separates
/// adjacent ranks, so operators can see at a glance how the group
/// breaks down without naming each dot.
export function RankedDotStrip({ members }: { members: FacilityServerStatus[] }) {
	const sorted = [...members].sort(compareServersByRankThenKind);
	const chunks: Array<{ rank: string; entries: FacilityServerStatus[] }> = [];
	for (const m of sorted) {
		const key = m.rank ?? "_unranked";
		const last = chunks[chunks.length - 1];
		if (last && last.rank === key) last.entries.push(m);
		else chunks.push({ rank: key, entries: [m] });
	}
	return (
		<Stack direction="row" spacing={0} sx={{ flexWrap: "wrap", alignItems: "center" }}>
			{chunks.map((chunk, idx) => (
				<Box
					key={chunk.rank}
					component="span"
					sx={{ display: "inline-flex", alignItems: "center" }}
				>
					{idx > 0 && (
						<Box
							component="span"
							aria-hidden
							sx={{
								display: "inline-block",
								width: "1px",
								height: "0.9em",
								mx: 0.5,
								bgcolor: "text.disabled",
							}}
						/>
					)}
					{chunk.entries.map((m) => (
						<Tooltip
							key={m.id}
							title={`${m.name || "(unnamed)"}${
								m.rank ? ` · ${m.rank}` : ""
							} · ${m.kind}`}
						>
							<Box component="span" sx={{ display: "inline-flex" }}>
								<StatusDot
									up={m.up}
									health={m.health}
									title={`${m.name}: ${m.up}${
										m.health !== "healthy" ? ` (${m.health})` : ""
									}`}
								/>
							</Box>
						</Tooltip>
					))}
				</Box>
			))}
		</Stack>
	);
}
