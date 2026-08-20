import CalendarMonthIcon from "@mui/icons-material/CalendarMonth";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import DeleteIcon from "@mui/icons-material/Delete";
import EditIcon from "@mui/icons-material/Edit";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import {
	Alert,
	Autocomplete,
	Box,
	Button,
	ButtonGroup,
	Chip,
	Collapse,
	createFilterOptions,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	IconButton,
	LinearProgress,
	MenuItem,
	Paper,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableRow,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import { useMemo, useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import TimeAgo from "../components/TimeAgo";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { usePageTitle } from "../hooks/usePageTitle";
import type { ApiResponse } from "../types";

type PastPlan = ApiResponse<"upgrade_plans", "history">[number];
type PlannableVersion = ApiResponse<"upgrade_plans", "targets">[number];

/// Where every deployment is going. A group with no plan is listed too: one
/// several minors behind with nothing recorded is what this view exists to
/// surface.
// spec: UPG#the-dashboard
export default function Upgrades() {
	usePageTitle("Upgrades");
	const isAdmin = useIsAdmin() === true;
	const [tick, setTick] = useState(0);
	const fleet = useApi("upgrade_plans", "fleet", {}, [tick]);
	const past = useApi("upgrade_plans", "history", {}, [tick]);

	if (fleet.status === "loading" || fleet.status === "idle") {
		return <LinearProgress />;
	}
	if (fleet.status === "error") {
		return <Alert severity="error">{fleet.error.message}</Alert>;
	}

	const planned = fleet.data.filter((row) => row.plan);
	const unplanned = fleet.data.filter((row) => !row.plan);

	return (
		<Stack spacing={2}>
			<Typography variant="h4" component="h1">
				Upgrades
			</Typography>

			{isAdmin && (
				<RecordPlan
					groups={fleet.data.map((row) => ({
						id: row.group_id,
						name: row.group_name,
					}))}
					onRecorded={() => setTick((t) => t + 1)}
				/>
			)}

			<PlanCalendar
				fleet={fleet.data}
				past={past.status === "ok" ? past.data : []}
			/>

			<Paper variant="outlined" sx={{ p: 2 }} data-testid="planned-upgrades">
				<Typography variant="h6" component="h2" gutterBottom>
					Planned
				</Typography>
				{planned.length === 0 ? (
					<Typography variant="body2" color="text.secondary">
						No deployment has a recorded plan.
					</Typography>
				) : (
					<Table size="small" sx={TIGHT_TABLE}>
						<TableHead>
							<TableRow>
								<TableCell>Deployment</TableCell>
								<TableCell>Running</TableCell>
								<TableCell>Going to</TableCell>
								<TableCell>Data survives it</TableCell>
								<TableCell>Planned for</TableCell>
								<TableCell>Time</TableCell>
								<TableCell>Note</TableCell>
								{isAdmin && <TableCell />}
							</TableRow>
						</TableHead>
						<TableBody>
							{planned.map((row) => (
								<TableRow
									key={row.group_id}
									data-testid="planned-upgrade-row"
								>
										<TableCell>
											<RouterLink to={`/groups/${row.group_id}`}>
												{row.group_name}
											</RouterLink>
										</TableCell>
										<TableCell>{row.current_version ?? "unknown"}</TableCell>
										<TableCell>{row.target_version}</TableCell>
										<TableCell>
											<Stack
												direction="row"
												spacing={0.5}
												sx={{ alignItems: "center" }}
											>
												<VerdictChip
													verdict={row.verdict}
													testable={row.testable}
												/>
												<AttemptChip attempt={row.attempt} />
											</Stack>
										</TableCell>
										<TableCell>
											<PlannedFor
												date={row.plan?.planned_for ?? null}
												late={row.late}
											/>
										</TableCell>
										<TableCell>
											<PlannedTime
												time={row.plan?.planned_time ?? null}
												zone={row.plan?.planned_zone ?? null}
											/>
										</TableCell>
										<TableCell>
											<PlanNote
												note={row.plan?.note ?? null}
												testId="planned-upgrade-note"
											/>
										</TableCell>
										{isAdmin && (
											<TableCell align="right">
												<EditPlan
													planId={row.plan?.id ?? ""}
													groupName={row.group_name}
													targetVersion={row.target_version ?? ""}
													plannedFor={row.plan?.planned_for ?? null}
													plannedTime={row.plan?.planned_time ?? null}
													plannedZone={row.plan?.planned_zone ?? null}
													note={row.plan?.note ?? null}
													onAmended={() => setTick((t) => t + 1)}
												/>
												<WithdrawPlan
													planId={row.plan?.id ?? ""}
													groupName={row.group_name}
													targetVersion={row.target_version ?? ""}
													onWithdrawn={() => setTick((t) => t + 1)}
												/>
											</TableCell>
										)}
								</TableRow>
							))}
						</TableBody>
					</Table>
				)}
			</Paper>

			<Disclosure
				title="No plan recorded"
				subject="deployments with no plan"
				caption={
					unplanned.length === 1
						? "1 deployment gets no pre-upgrade testing until a plan says where it is going"
						: `${unplanned.length} deployments get no pre-upgrade testing until a plan says where they are going`
				}
				testId="unplanned-upgrades"
			>
					{unplanned.length === 0 ? (
						<Typography variant="body2" color="text.secondary">
							Every deployment has a plan.
						</Typography>
					) : (
						<Table size="small" sx={TIGHT_TABLE}>
							<TableHead>
								<TableRow>
									<TableCell>Deployment</TableCell>
									<TableCell>Running</TableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{unplanned.map((row) => (
									<TableRow
										key={row.group_id}
										data-testid="unplanned-upgrade-row"
									>
										<TableCell>
											<RouterLink to={`/groups/${row.group_id}`}>
												{row.group_name}
											</RouterLink>
										</TableCell>
										<TableCell>{row.current_version ?? "unknown"}</TableCell>
									</TableRow>
								))}
							</TableBody>
						</Table>
					)}
			</Disclosure>

			<PastPlans plans={past.status === "ok" ? past.data : []} />
		</Stack>
	);
}

type FleetRow = ApiResponse<"upgrade_plans", "fleet">[number];

type Day = { date: string; entries: Entry[] };

/// How an entry reads at a glance: where a deployment is going, where it has
/// gone, and where the day it named has been and passed.
type Tone = "open" | "late" | "done";

type Entry = {
	key: string;
	date: string;
	groupId: string;
	group: string;
	version: string;
	tone: Tone;
};

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Green is this theme's primary, so a met plan owns it and one still ahead
/// takes the secondary blue: an operator reading the month should not have to
/// look twice to tell what has happened from what has not.
const TONES: Record<Tone, "secondary" | "warning" | "success"> = {
	open: "secondary",
	late: "warning",
	done: "success",
};

/// Beyond this a day's entries would crowd the week out of shape, so the rest
/// sit behind a count that names them.
const ENTRIES_PER_DAY = 3;

/// The month a deployment moves, read the way anyone reads a month. The table
/// below answers what each plan says; this answers which week is busy and which
/// two deployments land on the same night.
// spec: UPG#the-dashboard
function PlanCalendar({ fleet, past }: { fleet: FleetRow[]; past: PastPlan[] }) {
	const [monthsAhead, setMonthsAhead] = useState(0);
	const today = localDate(new Date());

	const entries = useMemo(() => {
		const all: Entry[] = [];
		for (const row of fleet) {
			if (!row.plan?.planned_for) continue;
			all.push({
				key: row.plan.id,
				date: row.plan.planned_for,
				groupId: row.group_id,
				group: row.group_name,
				version: row.target_version ?? "",
				tone: row.late ? "late" : "open",
			});
		}
		// Met plans stay on the calendar: what landed is half of what a month
		// of upgrades looked like.
		for (const row of past) {
			if (row.outcome !== "met" || !row.plan.planned_for) continue;
			all.push({
				key: row.plan.id,
				date: row.plan.planned_for,
				groupId: row.group_id,
				group: row.group_name,
				version: row.target_version,
				tone: "done",
			});
		}
		return all.sort((a, b) => a.group.localeCompare(b.group));
	}, [fleet, past]);

	const start = new Date();
	start.setDate(1);
	start.setMonth(start.getMonth() + monthsAhead);
	const month = localDate(start).slice(0, 7);
	const weeks = monthWeeks(start, entries);
	const shown = entries.filter((entry) => entry.date.startsWith(month)).length;

	return (
		<Paper variant="outlined" sx={{ p: 2 }} data-testid="upgrade-calendar">
			<Stack direction="row" spacing={1} sx={{ alignItems: "center", mb: 1.5 }}>
				<Box sx={{ flexGrow: 1, minWidth: 0 }}>
					<Typography variant="h6" component="h2" sx={{ lineHeight: 1.25 }}>
						{start.toLocaleDateString(undefined, { month: "long" })}{" "}
						<Box component="span" sx={{ color: "text.secondary", fontWeight: 400 }}>
							{start.getFullYear()}
						</Box>
					</Typography>
					<Typography variant="caption" color="text.secondary">
						{shown === 0
							? "nothing planned this month"
							: shown === 1
								? "1 upgrade this month"
								: `${shown} upgrades this month`}
					</Typography>
				</Box>
				<Button
					size="small"
					component={RouterLink}
					to="/settings/calendar-feeds"
					startIcon={<CalendarMonthIcon />}
				>
					Subscribe
				</Button>
				<ButtonGroup size="small" variant="outlined" sx={{ flexShrink: 0 }}>
					<Button
						aria-label="previous month"
						onClick={() => setMonthsAhead((m) => m - 1)}
					>
						<ChevronLeftIcon fontSize="small" />
					</Button>
					<Button onClick={() => setMonthsAhead(0)}>Today</Button>
					<Button
						aria-label="next month"
						onClick={() => setMonthsAhead((m) => m + 1)}
					>
						<ChevronRightIcon fontSize="small" />
					</Button>
				</ButtonGroup>
			</Stack>

			<Box sx={CALENDAR_GRID}>
				{WEEKDAYS.map((day) => (
					<Typography key={day} variant="caption" sx={WEEKDAY_CELL}>
						{day}
					</Typography>
				))}
				{weeks.flat().map((day) => {
					const inMonth = day.date.startsWith(month);
					const overflow = day.entries.slice(ENTRIES_PER_DAY);
					return (
						<Box
							key={day.date}
							data-testid="calendar-day"
							data-date={day.date}
							sx={(theme) => ({
								...CALENDAR_CELL,
								// Opaque: the 1px gridlines are the container's own
								// background showing through the gaps, so a tinted cell
								// would let them flood it.
								bgcolor: inMonth
									? "background.paper"
									: theme.palette.mode === "dark"
										? theme.palette.grey[900]
										: theme.palette.grey[50],
							})}
						>
							<Box sx={{ display: "flex", justifyContent: "flex-end" }}>
								<Box
									sx={{
										...DAY_NUMBER,
										color: inMonth ? "text.secondary" : "text.disabled",
										...(day.date === today ? TODAY_NUMBER : null),
									}}
								>
									{Number(day.date.slice(8))}
								</Box>
							</Box>
							{day.entries.slice(0, ENTRIES_PER_DAY).map((entry) => (
								<CalendarEntry key={entry.key} entry={entry} />
							))}
							{overflow.length > 0 && (
								<Tooltip
									title={overflow
										.map((entry) => `${entry.group} ${entry.version}`)
										.join(", ")}
								>
									<Typography variant="caption" sx={OVERFLOW_COUNT}>
										+{overflow.length} more
									</Typography>
								</Tooltip>
							)}
						</Box>
					);
				})}
			</Box>
		</Paper>
	);
}

function CalendarEntry({ entry }: { entry: Entry }) {
	const tone = TONES[entry.tone];
	const title =
		entry.tone === "done"
			? `${entry.group} reached ${entry.version}`
			: entry.tone === "late"
				? `${entry.group} to ${entry.version}, and the day has passed`
				: `${entry.group} to ${entry.version}`;

	return (
		<Tooltip title={title}>
			<Box
				component={RouterLink}
				to={`/groups/${entry.groupId}`}
				data-testid="calendar-entry"
				sx={(theme) => ({
					...CALENDAR_ENTRY,
					bgcolor: alpha(theme.palette[tone].main, 0.14),
					"&:hover": { bgcolor: alpha(theme.palette[tone].main, 0.26) },
				})}
			>
				<Box sx={{ ...ENTRY_BAR, bgcolor: `${tone}.main` }} />
				<Box sx={ENTRY_LABEL}>
					<Box component="span" sx={{ fontWeight: 500 }}>
						{entry.group}
					</Box>{" "}
					<Box component="span" sx={{ color: "text.secondary" }}>
						{entry.version}
					</Box>
				</Box>
			</Box>
		</Tooltip>
	);
}

/// The weeks covering `month`, Monday first, each day carrying the plans that
/// fall on it. Leading and trailing days belong to the neighbouring months and
/// are drawn faded rather than left blank, so a plan on the 1st or the 31st is
/// never off the edge of the view.
function monthWeeks(month: Date, entries: Entry[]): Day[][] {
	const first = new Date(month.getFullYear(), month.getMonth(), 1);
	const cursor = new Date(first);
	cursor.setDate(1 - ((first.getDay() + 6) % 7));

	const weeks: Day[][] = [];
	for (let week = 0; week < 6; week++) {
		const days: Day[] = [];
		for (let day = 0; day < 7; day++) {
			const date = localDate(cursor);
			days.push({ date, entries: entries.filter((e) => e.date === date) });
			cursor.setDate(cursor.getDate() + 1);
		}
		weeks.push(days);
		// A month spans six weeks only when it starts late enough to need one.
		if (cursor.getMonth() !== month.getMonth() && week >= 4) break;
	}
	return weeks;
}

function localDate(at: Date): string {
	return [
		at.getFullYear(),
		String(at.getMonth() + 1).padStart(2, "0"),
		String(at.getDate()).padStart(2, "0"),
	].join("-");
}

const CALENDAR_GRID = {
	display: "grid",
	gridTemplateColumns: "repeat(7, minmax(0, 1fr))",
	gap: "1px",
	bgcolor: "divider",
	border: 1,
	borderColor: "divider",
	borderRadius: 1,
	overflow: "hidden",
};

const WEEKDAY_CELL = {
	bgcolor: "background.paper",
	color: "text.secondary",
	textAlign: "center",
	textTransform: "uppercase",
	letterSpacing: "0.08em",
	fontSize: "0.65rem",
	py: 0.75,
};

const CALENDAR_CELL = {
	minHeight: 80,
	p: 0.5,
	display: "flex",
	flexDirection: "column",
	gap: 0.25,
	minWidth: 0,
};

const DAY_NUMBER = {
	width: 22,
	height: 22,
	display: "grid",
	placeItems: "center",
	fontSize: "0.7rem",
};

const TODAY_NUMBER = {
	bgcolor: "primary.dark",
	color: "common.white",
	borderRadius: "50%",
	fontWeight: 600,
};

const CALENDAR_ENTRY = {
	display: "flex",
	alignItems: "stretch",
	gap: 0.5,
	px: 0.5,
	py: "2px",
	borderRadius: 0.75,
	minWidth: 0,
	textDecoration: "none",
	color: "text.primary",
	fontSize: "0.68rem",
	lineHeight: 1.6,
};

const ENTRY_BAR = {
	width: 3,
	borderRadius: 3,
	flexShrink: 0,
};

const ENTRY_LABEL = {
	minWidth: 0,
	overflow: "hidden",
	textOverflow: "ellipsis",
	whiteSpace: "nowrap",
};

const OVERFLOW_COUNT = {
	color: "text.secondary",
	fontSize: "0.65rem",
	pl: 0.5,
	cursor: "default",
};

/// What each deployment planned before, and how it ended. A withdrawn plan is
/// readable here or nowhere: a deployment that stopped going somewhere leaves
/// no other mark on the fleet.
// spec: UPG#the-dashboard
function PastPlans({ plans }: { plans: PastPlan[] }) {
	if (plans.length === 0) return null;

	return (
		<Disclosure
			title="Past plans"
			subject="past plans"
			caption="where each deployment was going before, and how it ended"
			testId="past-plans"
		>
			<Table size="small" sx={TIGHT_TABLE}>
				<TableHead>
					<TableRow>
						<TableCell>Deployment</TableCell>
						<TableCell>Was going to</TableCell>
						<TableCell>Planned for</TableCell>
						<TableCell>Time</TableCell>
						<TableCell>Ended</TableCell>
						<TableCell>Note</TableCell>
					</TableRow>
				</TableHead>
				<TableBody>
					{plans.map((row) => (
						<TableRow key={row.plan.id} data-testid="past-plan-row">
								<TableCell>
									<RouterLink to={`/groups/${row.group_id}`}>
										{row.group_name}
									</RouterLink>
								</TableCell>
								<TableCell>{row.target_version}</TableCell>
								<TableCell>{row.plan.planned_for ?? ""}</TableCell>
								<TableCell>
									<PlannedTime
										time={row.plan.planned_time ?? null}
										zone={row.plan.planned_zone ?? null}
									/>
								</TableCell>
								<TableCell>
									<Stack
										direction="row"
										spacing={0.5}
										sx={{ alignItems: "center" }}
									>
										<OutcomeChip
											outcome={row.outcome}
											withdrawnBy={row.plan.withdrawn_by}
										/>
										<Typography variant="body2" color="text.secondary">
											<TimeAgo timestamp={row.ended_at} />
										</Typography>
									</Stack>
								</TableCell>
								<TableCell>
									<PlanNote
										note={row.plan.note}
										testId="past-plan-note"
									/>
								</TableCell>
							</TableRow>
					))}
				</TableBody>
			</Table>
		</Disclosure>
	);
}

/// How a plan ended. Withdrawn reads differently from met: the deployment
/// stopped going there rather than arriving.
function OutcomeChip({
	outcome,
	withdrawnBy,
}: {
	outcome: PastPlan["outcome"];
	withdrawnBy: string | null;
}) {
	if (outcome === "met") {
		return <Chip size="small" color="success" label="met" />;
	}
	if (outcome === "withdrawn") {
		return (
			<Tooltip
				title={
					withdrawnBy
						? `withdrawn by ${withdrawnBy}; the upgrade did not happen`
						: "the deployment stopped going there; the upgrade did not happen"
				}
			>
				<Chip size="small" color="warning" variant="outlined" label="withdrawn" />
			</Tooltip>
		);
	}
	return (
		<Tooltip title="a later plan took its place">
			<Chip size="small" variant="outlined" label="replaced" />
		</Tooltip>
	);
}

/// Whether the deployment's own data survives the planned version, rolled up
/// from its servers. Pairing it with the plan is the point of this view.
function VerdictChip({
	verdict,
	testable,
}: {
	verdict: string | null | undefined;
	testable: boolean | null | undefined;
}) {
	if (verdict === "passed") {
		return <Chip size="small" color="success" label="passed" />;
	}
	if (verdict === "failed") {
		return (
			<Tooltip title="a server's data broke the migrations; the version is held back">
				<Chip size="small" color="warning" label="failed" />
			</Tooltip>
		);
	}
	if (testable === false) {
		return (
			<Tooltip title="nothing is declared to migrate this deployment's data, so no test will run: declare a restore replica for it on the group's page">
				<Chip
					size="small"
					color="warning"
					variant="outlined"
					label="not set up"
				/>
			</Tooltip>
		);
	}
	return <Chip size="small" variant="outlined" label="not yet tested" />;
}

/// An attempt under way, beside the verdict rather than replacing it: a row can
/// read as failed with a fresh attempt already running.
function AttemptChip({
	attempt,
}: {
	attempt: "in_flight" | "ended_without_report" | null | undefined;
}) {
	if (attempt === "in_flight") {
		return (
			<Tooltip title="a restore is under way; a verdict lands when it reports">
				<Chip size="small" color="info" variant="outlined" label="testing" />
			</Tooltip>
		);
	}
	if (attempt === "ended_without_report") {
		return (
			<Tooltip title="a restore ran and never reported how it went, so the pipeline may be stuck">
				<Chip
					size="small"
					color="warning"
					variant="outlined"
					label="no report"
				/>
			</Tooltip>
		);
	}
	return null;
}

function PlannedFor({ date, late }: { date: string | null; late: boolean }) {
	if (!date) {
		return (
			<Typography variant="body2" color="text.secondary">
				no date
			</Typography>
		);
	}
	if (!late) {
		return <>{date}</>;
	}
	return (
		<Tooltip title="the planned day has passed and the deployment has not moved">
			<Chip size="small" color="warning" variant="outlined" label={`${date} (late)`} />
		</Tooltip>
	);
}

/// A section the operator opens when they want it. Both of these answer a
/// question that is worth having on the page and not worth reading every time.
function Disclosure({
	title,
	subject,
	caption,
	testId,
	children,
}: {
	title: string;
	subject: string;
	caption: string;
	testId: string;
	children: React.ReactNode;
}) {
	const [open, setOpen] = useState(false);
	return (
		<Paper variant="outlined" sx={{ p: 2 }} data-testid={testId}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "baseline", cursor: "pointer" }}
				onClick={() => setOpen((shown) => !shown)}
			>
				<IconButton
					size="small"
					aria-label={`${open ? "Hide" : "Show"} ${subject}`}
					aria-expanded={open}
				>
					{open ? (
						<ExpandLessIcon fontSize="small" />
					) : (
						<ExpandMoreIcon fontSize="small" />
					)}
				</IconButton>
				<Typography variant="h6" component="h2">
					{title}
				</Typography>
				<Typography variant="body2" color="text.secondary">
					{caption}
				</Typography>
			</Stack>
			<Collapse in={open}>{children}</Collapse>
		</Paper>
	);
}

/// What an operator needed the next reader to know, under the row it belongs
/// to. Held to one line so a long note cannot set the row height for every
/// other deployment; the whole of it is on hover.
function PlanNote({ note, testId }: { note: string | null; testId: string }) {
	if (!note) return null;
	return (
		<Tooltip title={note}>
			<Typography variant="body2" noWrap sx={NOTE_TEXT} data-testid={testId}>
				{note}
			</Typography>
		</Tooltip>
	);
}

/// The hour a deployment moves, as the wall clock it was recorded as. Canopy
/// holds no timezone for a group, so the zone travels with the time or the
/// reader cannot tell whose midnight it is.
function PlannedTime({
	time,
	zone,
}: {
	time: string | null;
	zone: string | null;
}) {
	if (!time || !zone) return null;
	const offset = zoneOffset(zone);
	return (
		<Tooltip title={offset ? `${zone} (${offset})` : zone}>
			<span>
				{clockTime(time)} {zoneLabel(zone)}
			</span>
		</Tooltip>
	);
}

function clockTime(time: string): string {
	const [hours, minutes] = time.split(":").map(Number);
	const suffix = hours < 12 ? "am" : "pm";
	const hour = hours % 12 === 0 ? 12 : hours % 12;
	if (minutes === 0) return `${hour}${suffix}`;
	return `${hour}:${String(minutes).padStart(2, "0")}${suffix}`;
}

/// tzdata dropped invented abbreviations in 2017, so these zones report
/// themselves as `+12` and CLDR only offers `GMT+12`. Australia and New Zealand
/// are absent on purpose: CLDR has real DST-aware ones for those.
const ZONE_ABBREVIATIONS: Record<string, string> = {
	"Pacific/Fiji": "FJT",
	"Pacific/Nauru": "NRT",
	"Pacific/Tarawa": "GILT",
	"Pacific/Guadalcanal": "SBT",
	"Pacific/Port_Moresby": "PGT",
	"Pacific/Efate": "VUT",
	"Pacific/Apia": "WST",
	"Pacific/Tongatapu": "TOT",
	"Pacific/Palau": "PWT",
	"Asia/Karachi": "PKT",
	"Asia/Dili": "TLT",
	"Indian/Maldives": "MVT",
};

/// What to call the zone in a table cell. CLDR first, since it is DST-aware
/// where it has an answer, then the region's own abbreviation, then the place.
function zoneLabel(zone: string): string {
	const short = zoneShortName(zone);
	if (short && !short.startsWith("GMT") && !short.startsWith("UTC")) {
		return short;
	}
	return (
		ZONE_ABBREVIATIONS[zone] ?? (zone.split("/").pop() ?? zone).replace(/_/g, " ")
	);
}

const zoneShortName = (zone: string) => zonePart(zone, "short");

const zoneOffset = (zone: string) => zonePart(zone, "shortOffset");

function zonePart(
	zone: string,
	timeZoneName: "short" | "shortOffset",
): string | null {
	try {
		return (
			new Intl.DateTimeFormat("en", { timeZone: zone, timeZoneName })
				.formatToParts(new Date())
				.find((part) => part.type === "timeZoneName")?.value ?? null
		);
	} catch {
		return null;
	}
}

const PLAN_FORM = {
	display: "grid",
	gridTemplateColumns:
		"minmax(0, 1.15fr) minmax(0, 1.15fr) 155px 136px 150px minmax(0, 1.2fr) auto",
	columnGap: 1.5,
	rowGap: 1,
	alignItems: "start",
};

const WHEN_FIELDS = {
	display: "grid",
	gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
	columnGap: 1.5,
};

const NOTE_TEXT = { maxWidth: 240 };

const TIGHT_TABLE = {
	"& td, & th": { width: "1%", whiteSpace: "nowrap" },
	"& td:last-child, & th:last-child": { width: "auto" },
};

const ZONES = Intl.supportedValuesOf("timeZone");

/// The zone most plans are in, so leaving it alone records what was meant.
const DEFAULT_ZONE = "Pacific/Fiji";

/// The zone a planned time is a wall clock in. Paired with the time field: a
/// plan with no hour records no zone.
function ZoneField({
	value,
	onChange,
	disabled,
}: {
	value: string;
	onChange: (zone: string) => void;
	disabled: boolean;
}) {
	return (
		<Autocomplete<string, false, true, false>
			size="small"
			disabled={disabled}
			disableClearable
			options={ZONES}
			value={value}
			onChange={(_, zone) => onChange(zone)}
			renderInput={(params) => <TextField {...params} label="Timezone" />}
		/>
	);
}

const SHORTLIST_MINORS = 10;

const matchVersion = createFilterOptions<PlannableVersion>({
	stringify: (option) => option.version,
});

/// The newest ready patch of each of the last ten minors. Every published
/// version ahead is plannable, which runs to hundreds, so the list an operator
/// scrolls is the recent releases and typing reaches the rest.
///
/// A minor with nothing clear of known issues still gets a row, flagged, rather
/// than dropping out of the list unexplained.
function recentMinors(options: PlannableVersion[]): PlannableVersion[] {
	const picked = new Map<string, PlannableVersion>();
	for (const option of options) {
		const minor = option.version.split(".").slice(0, 2).join(".");
		const held = picked.get(minor);
		if (held ? held.ready || !option.ready : picked.size === SHORTLIST_MINORS) {
			continue;
		}
		picked.set(minor, option);
	}
	return [...picked.values()];
}

function helperText(
	groupId: string,
	options: PlannableVersion[],
	shortlist: PlannableVersion[],
): string | undefined {
	if (!groupId) return undefined;
	if (options.length === 0) return "already on the newest";
	if (options.length > shortlist.length) return "type for older";
	return undefined;
}

/// Record where a deployment is going. The version picker offers only valid
/// targets, so the operator cannot pick one the API would refuse.
// spec: UPG#a-plan
function RecordPlan({
	groups,
	onRecorded,
}: {
	groups: Array<{ id: string; name: string }>;
	onRecorded: () => void;
}) {
	const [groupId, setGroupId] = useState("");
	const [versionId, setVersionId] = useState("");
	const [plannedFor, setPlannedFor] = useState("");
	const [plannedTime, setPlannedTime] = useState("");
	const [zone, setZone] = useState(DEFAULT_ZONE);
	const [note, setNote] = useState("");
	const record = useApiAction("upgrade_plans", "record");
	const targets = useApi(
		"upgrade_plans",
		"targets",
		groupId ? { group_id: groupId } : undefined,
		[groupId],
	);

	const options = targets.status === "ok" ? targets.data : [];
	const shortlist = useMemo(() => recentMinors(options), [options]);

	const submit = async () => {
		if (!groupId || !versionId) return;
		await record.call({
			group_id: groupId,
			target_version_id: versionId,
			planned_for: plannedFor || null,
			planned_time: plannedTime || null,
			planned_zone: plannedTime ? zone : null,
			note: note || null,
		});
		setVersionId("");
		setPlannedFor("");
		setPlannedTime("");
		setNote("");
		onRecorded();
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }} data-testid="record-plan">
			<Typography variant="h6" component="h2" gutterBottom>
				Record a plan
			</Typography>
			<Box sx={PLAN_FORM}>
					<TextField
						select
						size="small"
						label="Deployment"
						value={groupId}
						onChange={(e) => {
							setGroupId(e.target.value);
							setVersionId("");
						}}
					>
						{groups.map((group) => (
							<MenuItem key={group.id} value={group.id}>
								{group.name}
							</MenuItem>
						))}
					</TextField>
						<Autocomplete<PlannableVersion, false, false, false>
							size="small"
							disabled={!groupId || options.length === 0}
							options={options}
							value={options.find((option) => option.id === versionId) ?? null}
							onChange={(_, option) => setVersionId(option?.id ?? "")}
							getOptionLabel={(option) => option.version}
							isOptionEqualToValue={(a, b) => a.id === b.id}
							filterOptions={(all, state) =>
								state.inputValue === "" ? shortlist : matchVersion(all, state)
							}
							renderOption={(props, option) => (
								<li {...props} key={option.id}>
									<Stack
										direction="row"
										spacing={1}
										sx={{ alignItems: "center" }}
									>
										<span>{option.version}</span>
										{!option.ready && (
											<Chip
												size="small"
												color="warning"
												variant="outlined"
												label="known issue"
											/>
										)}
									</Stack>
							</li>
						)}
						renderInput={(params) => (
							<TextField
								{...params}
								label="Going to"
								helperText={helperText(groupId, options, shortlist)}
							/>
						)}
					/>
					<TextField
						size="small"
						type="date"
						label="Planned for"
						disabled={!groupId}
						value={plannedFor}
						onChange={(e) => {
							setPlannedFor(e.target.value);
							if (!e.target.value) setPlannedTime("");
						}}
						slotProps={{ inputLabel: { shrink: true } }}
					/>
					<TextField
						size="small"
						type="time"
						label="Time"
						value={plannedTime}
						disabled={!plannedFor}
						onChange={(e) => setPlannedTime(e.target.value)}
						slotProps={{ inputLabel: { shrink: true } }}
					/>
					<ZoneField value={zone} onChange={setZone} disabled={!plannedTime} />
					<TextField
						size="small"
						label="Note"
						disabled={!groupId}
						value={note}
						onChange={(e) => setNote(e.target.value)}
					/>
					<Button
						variant="contained"
						disabled={!groupId || !versionId || record.pending}
						onClick={submit}
					>
						Record
					</Button>
			</Box>
			{record.error && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{record.error.message}
				</Alert>
			)}
		</Paper>
	);
}

/// Amend an open plan's date and note. The target is deliberately absent:
/// moving a deployment somewhere else is a new plan, not a correction to this
/// one, so it goes through the record form.
// spec: UPG#a-plan
function EditPlan({
	planId,
	groupName,
	targetVersion,
	plannedFor,
	plannedTime,
	plannedZone,
	note,
	onAmended,
}: {
	planId: string;
	groupName: string;
	targetVersion: string;
	plannedFor: string | null;
	plannedTime: string | null;
	plannedZone: string | null;
	note: string | null;
	onAmended: () => void;
}) {
	const [open, setOpen] = useState(false);
	const [date, setDate] = useState("");
	const [time, setTime] = useState("");
	const [zone, setZone] = useState(DEFAULT_ZONE);
	const [text, setText] = useState("");
	const amend = useApiAction("upgrade_plans", "amend");

	// Re-read the plan on each open so a row refreshed underneath doesn't leave
	// the form showing what it held last time.
	const start = () => {
		setDate(plannedFor ?? "");
		setTime(plannedTime?.slice(0, 5) ?? "");
		setZone(plannedZone ?? DEFAULT_ZONE);
		setText(note ?? "");
		setOpen(true);
	};

	const save = async () => {
		await amend.call({
			id: planId,
			planned_for: date || null,
			planned_time: time || null,
			planned_zone: time ? zone : null,
			note: text || null,
		});
		setOpen(false);
		onAmended();
	};

	return (
		<>
			<IconButton
				size="small"
				aria-label={`Edit ${groupName}'s plan`}
				onClick={start}
				disabled={!planId}
			>
				<EditIcon fontSize="small" />
			</IconButton>
			<Dialog
				open={open}
				onClose={() => setOpen(false)}
				fullWidth
				maxWidth="sm"
				data-testid="edit-plan"
			>
				<DialogTitle>
					{groupName} &rarr; {targetVersion}
				</DialogTitle>
				<DialogContent>
					<Stack spacing={2} sx={{ mt: 1 }}>
						<Box sx={WHEN_FIELDS}>
							<TextField
								size="small"
								type="date"
								label="Planned for"
								value={date}
								onChange={(e) => {
									setDate(e.target.value);
									if (!e.target.value) setTime("");
								}}
								slotProps={{ inputLabel: { shrink: true } }}
							/>
							<TextField
								size="small"
								type="time"
								label="Time"
								value={time}
								disabled={!date}
								onChange={(e) => setTime(e.target.value)}
								slotProps={{ inputLabel: { shrink: true } }}
							/>
							<ZoneField value={zone} onChange={setZone} disabled={!time} />
						</Box>
						<TextField
							size="small"
							label="Note"
							value={text}
							onChange={(e) => setText(e.target.value)}
							multiline
							minRows={2}
						/>
						<Typography variant="body2" color="text.secondary">
							To move {groupName} to a different version, record a new plan
							instead. This one is kept as history.
						</Typography>
						{amend.error && (
							<Alert severity="error">{amend.error.message}</Alert>
						)}
					</Stack>
				</DialogContent>
				<DialogActions>
					<Button onClick={() => setOpen(false)}>Cancel</Button>
					<Button variant="contained" onClick={save} disabled={amend.pending}>
						Save
					</Button>
				</DialogActions>
			</Dialog>
		</>
	);
}

/// Withdraw a plan: the deployment is no longer going there. This does not say
/// the upgrade happened; Canopy closes a met plan on its own.
// spec: UPG#a-plan
function WithdrawPlan({
	planId,
	groupName,
	targetVersion,
	onWithdrawn,
}: {
	planId: string;
	groupName: string;
	targetVersion: string;
	onWithdrawn: () => void;
}) {
	const withdraw = useApiAction("upgrade_plans", "withdraw");
	const onClick = async () => {
		if (
			!window.confirm(
				`Withdraw ${groupName}'s plan to move to ${targetVersion}? Pre-upgrade testing stops for this deployment until a new plan is recorded.`,
			)
		)
			return;
		try {
			await withdraw.call({ id: planId });
			onWithdrawn();
		} catch {
			/* surfaced by the reload showing the plan still there */
		}
	};

	return (
		<IconButton
			size="small"
			aria-label={`Withdraw ${groupName}'s plan`}
			onClick={onClick}
			disabled={withdraw.pending || !planId}
		>
			<DeleteIcon fontSize="small" />
		</IconButton>
	);
}
