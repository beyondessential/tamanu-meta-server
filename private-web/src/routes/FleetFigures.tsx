import {
	Alert,
	Autocomplete,
	Box,
	Chip,
	IconButton,
	LinearProgress,
	MenuItem,
	Paper,
	Select,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	TextField,
	Tooltip,
	Typography,
	createFilterOptions,
} from "@mui/material";
import BarChartIcon from "@mui/icons-material/BarChart";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import SortByAlphaIcon from "@mui/icons-material/SortByAlpha";
import { useMemo, useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { useReloadInterval } from "../hooks/useReloadInterval";
import { useIsVersionTracked } from "../hooks/useProducts";
import { valueComparator } from "../lib/valueOrder";
import type { FleetServerDetailData, Product } from "../types";

/// How many distinct values a distribution card shows before collapsing the
/// rest. A field like `uptimeSecs` is near-unique across the fleet, and a
/// line per server tells an operator nothing about spread.
const TOP_VALUES = 8;

/// The release branch a Tamanu version belongs to: `2.54.3` → `2.54`. The
/// fleet moves a release at a time, so the branch is the grouping an
/// operator acts on; the exact version splits it finer than that.
// spec: FIG#fleet-spread
function tamanuRelease(version: string | null): string | null {
	const parts = /^(\d+)\.(\d+)/.exec(version ?? "");
	return parts && `${parts[1]}.${parts[2]}`;
}

/// PostgreSQL's own major version, which the project renumbered at 10: from
/// there it's the leading component (`16.3` → `16`), and before it the first
/// two (`9.6.24` → `9.6`).
// spec: FIG#fleet-spread
function postgresMajor(version: string | null): string | null {
	const parts = /^(\d+)(?:\.(\d+))?/.exec(version ?? "");
	if (!parts) return null;
	if (Number(parts[1]) >= 10 || parts[2] === undefined) return parts[1]!;
	return `${parts[1]}.${parts[2]}`;
}

interface Figure {
	key: string;
	label: string;
	/// Whether the figure leads the page as a card of its own. The rest are
	/// reachable through the field lookup and the crossing.
	card?: boolean;
	/// Reads the value off a fleet row; everything else comes out of the raw
	/// payload.
	pick: (s: FleetServerDetailData) => unknown;
}

/// The figures canopy derives. Each version figure carries a coarser
/// grouping alongside the exact value it reports, and the coarse one is what
/// the page leads with.
// spec: FIG#fleet-spread
const FIGURES: Figure[] = [
	{
		key: "release",
		label: "Tamanu release",
		card: true,
		pick: (s) => tamanuRelease(s.version),
	},
	{ key: "version", label: "Tamanu version", pick: (s) => s.version },
	{
		key: "postgresMajor",
		label: "PostgreSQL major",
		card: true,
		pick: (s) => postgresMajor(s.postgres),
	},
	{ key: "postgres", label: "PostgreSQL version", pick: (s) => s.postgres },
	{ key: "bestool", label: "bestool", card: true, pick: (s) => s.bestool },
	{ key: "platform", label: "Platform", card: true, pick: (s) => s.platform },
	{ key: "nodejs", label: "Node.js", card: true, pick: (s) => s.nodejs },
	{ key: "timezone", label: "Timezone", card: true, pick: (s) => s.timezone },
];

/// The fields whose spread covers only the servers whose product canopy holds
/// a release train for. A server that has no application version to report is
/// absent from these spreads rather than counted among those reporting nothing
/// — it isn't a server that failed to report, it's a server with nothing to
/// report. Every other field, including the database-engine and bestool
/// versions, still covers the whole fleet.
// spec: APP#versions
const VERSION_TRACKED_FIELDS = new Set(["release", "version"]);

/// Narrow a server list to the ones a field's spread covers.
function coveredBy(
	field: string,
	servers: FleetServerDetailData[],
	isVersionTracked: (product: Product) => boolean,
): FleetServerDetailData[] {
	if (!VERSION_TRACKED_FIELDS.has(field)) return servers;
	return servers.filter((s) => isVersionTracked(s.product));
}

/// The name to present a field under: a figure's own label, or the field
/// name as an operator typed it.
function fieldLabel(field: string): string {
	return FIGURES.find((f) => f.key === field)?.label ?? field;
}

/// Read one healthcheck field off a fleet row. Check names can contain dots
/// themselves, so every split point is tried, longest check name first: for
/// `pg.replication.lag`, a check literally named `pg.replication` wins over
/// one named `pg`.
function pickCheckField(server: FleetServerDetailData, field: string): unknown {
	const checks = (server.checks ?? {}) as Record<string, Record<string, unknown> | undefined>;
	for (let dot = field.lastIndexOf("."); dot > 0; dot = field.lastIndexOf(".", dot - 1)) {
		const check = checks[field.slice(0, dot)];
		if (check) return check[field.slice(dot + 1)];
	}
	return undefined;
}

/// Resolve a field name to the value it takes on a fleet row: one of the
/// derived figures, a healthcheck's field as `check.field`, or a field the
/// sources report server-wide.
function picker(field: string): (s: FleetServerDetailData) => unknown {
	const figure = FIGURES.find((f) => f.key === field);
	if (figure) return figure.pick;
	const server = (s: FleetServerDetailData) => (s.detail as Record<string, unknown>)?.[field];
	if (!field.includes(".")) return server;
	// A server-wide field can carry a dot in its name too, so fall back to
	// one where no check answers to the part before the dot.
	return (s) => {
		const onCheck = pickCheckField(s, field);
		return onCheck === undefined ? server(s) : onCheck;
	};
}

/// The label a value groups under. Everything is compared as its rendered
/// text, so `16.3` and `"16.3"` land in the same bucket; `null` means the
/// server reports nothing for this field.
function bucket(value: unknown): string | null {
	if (value == null) return null;
	if (typeof value === "string") return value === "" ? null : value;
	if (typeof value === "object") return JSON.stringify(value);
	return String(value);
}

interface Group {
	value: string | null;
	servers: FleetServerDetailData[];
}

/// Group servers by their value for one field, largest group first, with
/// the unreported group always last so it doesn't crowd out real values.
function distribution(
	servers: FleetServerDetailData[],
	pick: (s: FleetServerDetailData) => unknown,
): Group[] {
	const byValue = new Map<string | null, FleetServerDetailData[]>();
	for (const server of servers) {
		const key = bucket(pick(server));
		const existing = byValue.get(key);
		if (existing) existing.push(server);
		else byValue.set(key, [server]);
	}
	return [...byValue.entries()]
		.map(([value, list]) => ({ value, servers: list }))
		.sort((a, b) => {
			if ((a.value === null) !== (b.value === null)) return a.value === null ? 1 : -1;
			return b.servers.length - a.servers.length || String(a.value).localeCompare(String(b.value));
		});
}

/// Whether a spread reads largest group first, or in the order of the values
/// themselves.
type SortMode = "popularity" | "value";

/// Reorder a spread that [`distribution`] has already put in popularity
/// order. Sorting by value compares the values as whatever they look like —
/// numbers, versions, or text. The unreported group stays last either way:
/// it's a population rather than a value, and sorts nowhere meaningful among
/// them.
// spec: FIG#fleet-spread
function orderGroups(groups: Group[], mode: SortMode): Group[] {
	if (mode === "popularity") return groups;
	const values = groups
		.map((g) => g.value)
		.filter((v): v is string => v !== null);
	const compare = valueComparator(values);
	return [...groups].sort((a, b) => {
		if (a.value === null || b.value === null) {
			return a.value === b.value ? 0 : a.value === null ? 1 : -1;
		}
		return compare(a.value, b.value);
	});
}

/// Flips a spread between its two orders. Shows what a click does rather
/// than where things stand, since the order itself is already on screen.
function SortToggle({
	mode,
	onChange,
}: {
	mode: SortMode;
	onChange: (mode: SortMode) => void;
}) {
	const next: SortMode = mode === "popularity" ? "value" : "popularity";
	const label = next === "value" ? "Sort by value" : "Sort by popularity";
	return (
		<Tooltip title={label}>
			<IconButton size="small" aria-label={label} onClick={() => onChange(next)}>
				{next === "value" ? (
					<SortByAlphaIcon fontSize="inherit" />
				) : (
					<BarChartIcon fontSize="inherit" />
				)}
			</IconButton>
		</Tooltip>
	);
}

export default function FleetFigures() {
	usePageTitle("Fleet figures");
	const tick = useReloadInterval(60_000, "canopy-data-changed");
	const result = useApi("statuses", "fleet_detail", {}, [tick]);
	const isVersionTracked = useIsVersionTracked();

	const servers = result.status === "ok" ? result.data : [];

	// Every field any source currently reports, so an operator can find one
	// without knowing its name. Healthcheck fields come through as
	// `check.field` — the same lookup, addressed through the check that
	// reports them.
	const reportedKeys = useMemo(() => {
		const keys = new Set<string>();
		const checkKeys = new Set<string>();
		for (const server of servers) {
			for (const key of Object.keys(server.detail ?? {})) keys.add(key);
			for (const [check, fields] of Object.entries(server.checks ?? {})) {
				for (const key of Object.keys((fields ?? {}) as object)) {
					checkKeys.add(`${check}.${key}`);
				}
			}
		}
		return [...[...keys].sort(), ...[...checkKeys].sort()];
	}, [servers]);

	if (result.status === "loading" || result.status === "idle") {
		return <LinearProgress />;
	}
	if (result.status === "error") {
		return <Alert severity="error">{result.error.message}</Alert>;
	}
	if (servers.length === 0) {
		return <Alert severity="info">No servers yet.</Alert>;
	}

	return (
		<Stack spacing={3}>
			<Typography variant="body2" color="text.secondary">
				What the fleet's {servers.length} servers currently report about
				themselves. Each value is the most recent one any source reported,
				so a server that has gone quiet still counts.
			</Typography>

			<Box
				sx={{
					display: "grid",
					gap: 2,
					gridTemplateColumns: {
						xs: "1fr",
						sm: "repeat(2, 1fr)",
						md: "repeat(3, 1fr)",
					},
				}}
			>
				{FIGURES.filter((figure) => figure.card).map((figure) => {
					// Each card's total is the population it actually covers, so
					// the version cards read against the servers that have a
					// version rather than the whole fleet.
					// spec: APP#versions
					const covered = coveredBy(figure.key, servers, isVersionTracked);
					return (
						<DistributionCard
							key={figure.key}
							label={figure.label}
							groups={distribution(covered, figure.pick)}
							total={covered.length}
						/>
					);
				})}
			</Box>

			<LookupCard servers={servers} keys={reportedKeys} />
			<CrossTab servers={servers} keys={reportedKeys} />
		</Stack>
	);
}

function DistributionCard({
	label,
	groups,
	total,
}: {
	label: string;
	groups: Group[];
	total: number;
}) {
	const [expanded, setExpanded] = useState<string | null>(null);
	const [showAll, setShowAll] = useState(false);
	const [sort, setSort] = useState<SortMode>("popularity");

	// Which values are worth a line is a question of size, so the collapse
	// takes the largest groups whichever order they end up presented in.
	const top = showAll ? groups : groups.slice(0, TOP_VALUES);
	const shown = orderGroups(top, sort);
	const hidden = groups.length - top.length;
	const hiddenServers = groups
		.slice(top.length)
		.reduce((sum, g) => sum + g.servers.length, 0);

	return (
		<Paper variant="outlined" sx={{ p: 2 }} role="group" aria-label={label}>
			<Stack
				direction="row"
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="subtitle2">{label}</Typography>
				<SortToggle mode={sort} onChange={setSort} />
			</Stack>
			<Stack spacing={0.5}>
				{shown.map((group) => (
					<ValueRow
						key={group.value ?? " unreported"}
						group={group}
						total={total}
						expanded={expanded === (group.value ?? " unreported")}
						onToggle={() =>
							setExpanded((v) =>
								v === (group.value ?? " unreported")
									? null
									: (group.value ?? " unreported"),
							)
						}
					/>
				))}
				{hidden > 0 && (
					<Typography
						variant="caption"
						color="text.secondary"
						sx={{ cursor: "pointer" }}
						onClick={() => setShowAll(true)}
					>
						{hidden} other {hidden === 1 ? "value" : "values"} ({hiddenServers}{" "}
						{hiddenServers === 1 ? "server" : "servers"}) — show all
					</Typography>
				)}
			</Stack>
		</Paper>
	);
}

function ValueRow({
	group,
	total,
	expanded,
	onToggle,
}: {
	group: Group;
	total: number;
	expanded: boolean;
	onToggle: () => void;
}) {
	const share = total === 0 ? 0 : (group.servers.length / total) * 100;
	return (
		<Box>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", cursor: "pointer" }}
				onClick={onToggle}
				role="button"
				aria-label={`${group.value ?? "not reported"}: ${group.servers.length}`}
			>
				<Typography
					variant="body2"
					sx={{
						fontFamily: group.value === null ? undefined : "monospace",
						color: group.value === null ? "text.secondary" : undefined,
						minWidth: 0,
						flex: "0 1 auto",
						overflow: "hidden",
						textOverflow: "ellipsis",
						whiteSpace: "nowrap",
					}}
				>
					{group.value ?? "not reported"}
				</Typography>
				<Box
					sx={{
						flex: "1 1 auto",
						height: 6,
						borderRadius: 1,
						bgcolor: "action.hover",
						overflow: "hidden",
						minWidth: 24,
					}}
				>
					<Box
						sx={{
							width: `${share}%`,
							height: "100%",
							bgcolor: group.value === null ? "action.disabled" : "primary.main",
						}}
					/>
				</Box>
				<Typography variant="body2" sx={{ flexShrink: 0 }}>
					{group.servers.length}
				</Typography>
				{expanded ? (
					<ExpandLessIcon fontSize="small" />
				) : (
					<ExpandMoreIcon fontSize="small" />
				)}
			</Stack>
			{expanded && <ServerChips servers={group.servers} />}
		</Box>
	);
}

function ServerChips({ servers }: { servers: FleetServerDetailData[] }) {
	return (
		<Stack direction="row" spacing={0.5} sx={{ flexWrap: "wrap", py: 0.5 }} useFlexGap>
			{servers.map((server) => (
				<Chip
					key={server.server_id}
					size="small"
					variant="outlined"
					label={server.server_name || server.server_id}
					component={RouterLink}
					to={`/servers/${server.server_id}`}
					clickable
				/>
			))}
		</Stack>
	);
}

function LookupCard({
	servers,
	keys,
}: {
	servers: FleetServerDetailData[];
	keys: string[];
}) {
	const [field, setField] = useState<string | null>(null);
	const isVersionTracked = useIsVersionTracked();
	const groups = useMemo(
		() =>
			field === null
				? []
				: distribution(
						coveredBy(field, servers, isVersionTracked),
						picker(field),
					),
		[servers, field, isVersionTracked],
	);
	// The figures the page doesn't lead with — the exact versions behind the
	// coarse groupings, mostly — are only reachable here and in the crossing.
	const options = useMemo(() => [...FIGURES.map((f) => f.key), ...keys], [keys]);

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="subtitle2" sx={{ mb: 1 }}>
				Look up a field
			</Typography>
			<Autocomplete
				freeSolo
				options={options}
				filterOptions={filterFields}
				groupBy={(option) =>
					FIGURES.some((f) => f.key === option)
						? "Figures"
						: option.includes(".")
							? "Healthcheck fields"
							: "Server fields"
				}
				renderOption={({ key, ...props }, option) => (
					<li key={key} {...props}>
						{fieldLabel(option)}
					</li>
				)}
				inputValue={field ?? ""}
				onInputChange={(_, v) => setField(v || null)}
				renderInput={(params) => (
					<TextField
						{...params}
						size="small"
						label="Field"
						placeholder="e.g. uptimeSecs or diskspace.percent"
					/>
				)}
				sx={{ maxWidth: 360, mb: field ? 2 : 0 }}
			/>
			{field && groups.length > 0 && (
				<DistributionCard
					label={fieldLabel(field)}
					groups={groups}
					total={servers.length}
				/>
			)}
		</Paper>
	);
}

/// Match a figure on the name it presents under as well as on the field name
/// it resolves to, so "Tamanu release" finds `release`.
const filterFields = createFilterOptions<string>({
	stringify: (option) => `${option} ${fieldLabel(option)}`,
});

function CrossTab({
	servers,
	keys,
}: {
	servers: FleetServerDetailData[];
	keys: string[];
}) {
	const fields = useMemo(
		() => [...new Set([...FIGURES.map((f) => f.key), ...keys])],
		[keys],
	);
	const [rowField, setRowField] = useState("postgresMajor");
	const [colField, setColField] = useState("release");
	const [cell, setCell] = useState<string | null>(null);
	const [sort, setSort] = useState<SortMode>("popularity");

	const isVersionTracked = useIsVersionTracked();
	const { rows, cols, counts } = useMemo(() => {
		const pickRow = picker(rowField);
		const pickCol = picker(colField);
		// A server absent from either axis is dropped from the table rather
		// than placed in an unreported row, so a crossing never implies it
		// failed to report a figure it doesn't have.
		// spec: APP#versions
		const covered = coveredBy(
			colField,
			coveredBy(rowField, servers, isVersionTracked),
			isVersionTracked,
		);
		const rowGroups = orderGroups(distribution(covered, pickRow), sort);
		const colGroups = orderGroups(distribution(covered, pickCol), sort);
		const counts = new Map<string, FleetServerDetailData[]>();
		for (const server of covered) {
			const key = `${bucket(pickRow(server))} ${bucket(pickCol(server))}`;
			const existing = counts.get(key);
			if (existing) existing.push(server);
			else counts.set(key, [server]);
		}
		return {
			rows: rowGroups.map((g) => g.value),
			cols: colGroups.map((g) => g.value),
			counts,
		};
	}, [servers, rowField, colField, sort, isVersionTracked]);

	const label = (value: string | null) => value ?? "not reported";

	return (
		<Paper variant="outlined" sx={{ p: 2 }} role="group" aria-label="Cross two fields">
			<Stack
				direction="row"
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="subtitle2">Cross two fields</Typography>
				<SortToggle mode={sort} onChange={setSort} />
			</Stack>
			<Stack direction="row" spacing={2} sx={{ mb: 2, flexWrap: "wrap" }} useFlexGap>
				<FieldPicker label="Rows" value={rowField} options={fields} onChange={setRowField} />
				<FieldPicker
					label="Columns"
					value={colField}
					options={fields}
					onChange={setColField}
				/>
			</Stack>
			<TableContainer sx={{ overflowX: "auto" }}>
				<Table size="small">
					<TableHead>
						<TableRow>
							<TableCell />
							{cols.map((col) => (
								<TableCell key={label(col)} align="right">
									{label(col)}
								</TableCell>
							))}
						</TableRow>
					</TableHead>
					<TableBody>
						{rows.map((row) => (
							<TableRow key={label(row)}>
								<TableCell component="th" scope="row">
									{label(row)}
								</TableCell>
								{cols.map((col) => {
									const key = `${row} ${col}`;
									const list = counts.get(key) ?? [];
									return (
										<TableCell
											key={label(col)}
											align="right"
											onClick={() =>
												list.length > 0 && setCell((c) => (c === key ? null : key))
											}
											sx={{
												cursor: list.length > 0 ? "pointer" : undefined,
												color: list.length === 0 ? "text.disabled" : undefined,
												bgcolor: cell === key ? "action.selected" : undefined,
											}}
										>
											{list.length || "·"}
										</TableCell>
									);
								})}
							</TableRow>
						))}
					</TableBody>
				</Table>
			</TableContainer>
			{cell && <ServerChips servers={counts.get(cell) ?? []} />}
		</Paper>
	);
}

function FieldPicker({
	label,
	value,
	options,
	onChange,
}: {
	label: string;
	value: string;
	options: string[];
	onChange: (v: string) => void;
}) {
	return (
		<Stack spacing={0.5}>
			<Typography variant="caption" color="text.secondary">
				{label}
			</Typography>
			<Select
				size="small"
				value={value}
				onChange={(e) => onChange(e.target.value)}
				inputProps={{ "aria-label": label }}
			>
				{options.map((option) => (
					<MenuItem key={option} value={option}>
						{fieldLabel(option)}
					</MenuItem>
				))}
			</Select>
		</Stack>
	);
}
