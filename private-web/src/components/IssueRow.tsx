import {
	Alert as MuiAlert,
	Box,
	Button,
	Chip,
	IconButton,
	Link as MuiLink,
	MenuItem,
	Stack,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import CheckCircleOutlinedIcon from "@mui/icons-material/CheckCircleOutlined";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import HistoryIcon from "@mui/icons-material/History";
import SnoozeIcon from "@mui/icons-material/Snooze";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { humanDuration } from "../lib/humanDuration";
import NotesList, { AddNoteButton } from "./NotesList";
import SeverityChip from "./SeverityChip";
import SourceChip from "./SourceChip";
import TimeAgo from "./TimeAgo";
import UserAvatar from "./UserAvatar";
import {
	RESOLVED_REASONS,
	RESOLVED_REASON_LABEL,
	type IssueData,
	type IssueIncidentLink,
	type ResolvedReason,
} from "../types";

function isSnoozeActive(snoozedUntil: string | null): boolean {
	if (!snoozedUntil) return false;
	return Date.parse(snoozedUntil) > Date.now();
}

function serverLabel(name: string | null, host: string): string {
	if (name && name.trim() !== "") return name;
	if (host && host.trim() !== "") return host;
	return "(unknown)";
}

function headline(issue: IssueData): string {
	if (issue.description && issue.description.trim() !== "") {
		return issue.description;
	}
	return issue.message.split("\n")[0] ?? "";
}

/** One issue. Expanded by default: shows message body, action buttons and
 * a two-column Events / Notes panel. When collapsed, only the header row
 * is visible — even the action buttons are hidden. Header layout:
 * `[toggle] [server?] [headline] [ack/avatar] [source] [severity] [time]`.
 * The headline is struck through when the issue is inactive or resolved.
 */
export default function IssueRow({
	issue,
	showServer = false,
	defaultExpanded = false,
	onChanged,
}: {
	issue: IssueData;
	showServer?: boolean;
	/** Initial expanded state. Default `false` (collapsed) — fits list views;
	 * incident-detail timelines pass `true`. */
	defaultExpanded?: boolean;
	onChanged: () => void;
}) {
	const [expanded, setExpanded] = useState(defaultExpanded);
	const [notesRefresh, setNotesRefresh] = useState(0);
	const snoozeActive = isSnoozeActive(issue.snoozed_until);
	const struckThrough = !issue.active || !!issue.resolved_at;
	const isAdmin = useIsAdmin() === true;
	return (
		<Box
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
				bgcolor: issue.active && !snoozeActive ? undefined : "action.hover",
			}}
		>
			<Header
				issue={issue}
				expanded={expanded}
				setExpanded={setExpanded}
				snoozeActive={snoozeActive}
				struckThrough={struckThrough}
				showServer={showServer}
				isAdmin={isAdmin}
				onChanged={onChanged}
			/>
			{expanded && (
				<Body
					issue={issue}
					snoozeActive={snoozeActive}
					notesRefresh={notesRefresh}
					isAdmin={isAdmin}
					onNoteAdded={() => setNotesRefresh((t) => t + 1)}
					onChanged={onChanged}
				/>
			)}
		</Box>
	);
}

function Header({
	issue,
	expanded,
	setExpanded,
	snoozeActive,
	struckThrough,
	showServer,
	isAdmin,
	onChanged,
}: {
	issue: IssueData;
	expanded: boolean;
	setExpanded: (v: (p: boolean) => boolean) => void;
	snoozeActive: boolean;
	struckThrough: boolean;
	showServer: boolean;
	isAdmin: boolean;
	onChanged: () => void;
}) {
	return (
		<Stack
			direction="row"
			spacing={1}
			sx={{ alignItems: "center", minWidth: 0 }}
		>
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
			{showServer && (
				<MuiLink
					component={RouterLink}
					to={`/servers/${issue.server_id}`}
					underline="hover"
					color="text.primary"
					sx={{ fontWeight: 500, flexShrink: 0 }}
				>
					{serverLabel(issue.server_name, issue.server_host)}
				</MuiLink>
			)}
			<Typography
				variant="body2"
				sx={{
					flex: 1,
					minWidth: 0,
					overflow: "hidden",
					textOverflow: "ellipsis",
					whiteSpace: "nowrap",
					textDecoration: struckThrough ? "line-through" : undefined,
					color: struckThrough ? "text.secondary" : undefined,
				}}
				title={headline(issue)}
			>
				{headline(issue)}
			</Typography>
			{snoozeActive && (
				<Typography
					variant="caption"
					color="warning.main"
					sx={{ flexShrink: 0 }}
					title={`snoozed until ${issue.snoozed_until}`}
				>
					snoozed
				</Typography>
			)}
			<IncidentLinks incidents={issue.incidents} />
			<HeaderActor issue={issue} isAdmin={isAdmin} onChanged={onChanged} />
			<Box sx={{ flexShrink: 0 }}>
				<SourceChip source={issue.source} refValue={issue.ref} />
			</Box>
			<Box sx={{ flexShrink: 0 }}>
				<SeverityChip severity={issue.severity} />
			</Box>
			<Typography
				variant="body2"
				color="text.secondary"
				sx={{ flexShrink: 0 }}
			>
				<ClosureOrTime issue={issue} />
			</Typography>
		</Stack>
	);
}

/** Header time slot. For closed issues, gives the closure context — reason
 * (human-resolved) or "on its own" (device sent inactive) — plus the
 * lifetime. For still-active issues, falls back to last-seen. */
function ClosureOrTime({ issue }: { issue: IssueData }) {
	if (issue.resolved_at) {
		const reasonKey = issue.resolved_reason as ResolvedReason | null;
		const reasonLabel =
			reasonKey && reasonKey in RESOLVED_REASON_LABEL
				? RESOLVED_REASON_LABEL[reasonKey].toLowerCase()
				: issue.resolved_reason;
		const lasted = humanDuration(issue.first_seen, issue.resolved_at);
		return (
			<>
				closed{reasonLabel ? ` as ${reasonLabel}` : ""}{" "}
				<TimeAgo timestamp={issue.resolved_at} />, lasted {lasted}
			</>
		);
	}
	if (!issue.active) {
		const lasted = humanDuration(issue.first_seen, issue.last_seen);
		return (
			<>
				closed on its own <TimeAgo timestamp={issue.last_seen} />, lasted {lasted}
			</>
		);
	}
	return <TimeAgo timestamp={issue.last_seen} />;
}

/** Leftmost slot of the right-side header cluster. Avatar (resolver or
 * acker) when applicable; an Ack button otherwise (admins only). There is
 * no Unack in the UI — the backend still supports it. */
function HeaderActor({
	issue,
	isAdmin,
	onChanged,
}: {
	issue: IssueData;
	isAdmin: boolean;
	onChanged: () => void;
}) {
	const ack = useApiAction("issues", "ack");
	const doAck = async () => {
		try {
			await ack.call({ issue_id: issue.id });
			onChanged();
		} catch {
			/* surfaced via ack.error inside Body */
		}
	};
	if (issue.resolved_at) {
		return (
			<Tooltip
				title={`resolved (${issue.resolved_reason ?? "?"}) by ${
					issue.resolved_by_name ?? issue.resolved_by ?? "?"
				}`}
			>
				<span>
					<UserAvatar
						login={issue.resolved_by}
						name={issue.resolved_by_name}
						profilePic={issue.resolved_by_pic}
					/>
				</span>
			</Tooltip>
		);
	}
	if (issue.acknowledged_at) {
		return (
			<Tooltip
				title={`acked by ${issue.acknowledged_by_name ?? issue.acknowledged_by ?? "?"}`}
			>
				<span>
					<UserAvatar
						login={issue.acknowledged_by}
						name={issue.acknowledged_by_name}
						profilePic={issue.acknowledged_by_pic}
					/>
				</span>
			</Tooltip>
		);
	}
	if (!isAdmin) return null;
	return (
		<Button
			size="small"
			variant="outlined"
			onClick={doAck}
			disabled={ack.pending}
		>
			Ack
		</Button>
	);
}

function Body({
	issue,
	snoozeActive,
	notesRefresh,
	isAdmin,
	onNoteAdded,
	onChanged,
}: {
	issue: IssueData;
	snoozeActive: boolean;
	notesRefresh: number;
	isAdmin: boolean;
	onNoteAdded: () => void;
	onChanged: () => void;
}) {
	return (
		<Box sx={{ mt: 1 }}>
			<Typography
				variant="body2"
				component="pre"
				sx={{
					m: 0,
					whiteSpace: "pre-wrap",
					fontFamily: "monospace",
					fontSize: "0.85em",
				}}
			>
				{issue.message}
			</Typography>
			{isAdmin && (
				<IssueActions
					issue={issue}
					snoozeActive={snoozeActive}
					onNoteAdded={onNoteAdded}
					onChanged={onChanged}
				/>
			)}
			<Box
				sx={{
					mt: 1.5,
					display: "grid",
					gridTemplateColumns: { xs: "1fr", sm: "1fr 1fr" },
					gap: 2,
				}}
			>
				<EventLog issueId={issue.id} />
				<NotesList
					apiModule="issues"
					parentKey="issue_id"
					parentId={issue.id}
					refreshKey={notesRefresh}
					canEdit={isAdmin}
				/>
			</Box>
		</Box>
	);
}

function IssueActions({
	issue,
	snoozeActive,
	onNoteAdded,
	onChanged,
}: {
	issue: IssueData;
	snoozeActive: boolean;
	onNoteAdded: () => void;
	onChanged: () => void;
}) {
	const resolve = useApiAction("issues", "resolve");
	const unresolve = useApiAction("issues", "unresolve");
	const snooze = useApiAction("issues", "snooze");
	const unsnooze = useApiAction("issues", "unsnooze");

	const [resolveOpen, setResolveOpen] = useState(false);
	const [snoozeOpen, setSnoozeOpen] = useState(false);
	const [reason, setReason] = useState<ResolvedReason>("fixed");
	const [snoozeHours, setSnoozeHours] = useState(4);

	const wrap = async (fn: () => Promise<unknown>) => {
		try {
			await fn();
			onChanged();
		} catch {
			/* surfaced via *.error */
		}
	};

	const error =
		resolve.error ?? unresolve.error ?? snooze.error ?? unsnooze.error;

	return (
		<Box sx={{ mt: 1 }}>
			<Stack direction="row" spacing={1} sx={{ flexWrap: "wrap" }} useFlexGap>
				{issue.resolved_at ? (
					<Button
						size="small"
						variant="outlined"
						color="warning"
						startIcon={<CheckCircleOutlinedIcon />}
						onClick={() => wrap(() => unresolve.call({ issue_id: issue.id }))}
					>
						Unresolve
					</Button>
				) : (
					<Tooltip
						title={issue.acknowledged_at ? "" : "Ack the issue first"}
						disableHoverListener={!!issue.acknowledged_at}
					>
						<span>
							<Button
								size="small"
								variant="outlined"
								color="success"
								startIcon={<CheckCircleOutlinedIcon />}
								disabled={!issue.acknowledged_at}
								onClick={() => setResolveOpen((v) => !v)}
							>
								Resolve…
							</Button>
						</span>
					</Tooltip>
				)}
				{snoozeActive ? (
					<Button
						size="small"
						variant="outlined"
						color="warning"
						startIcon={<SnoozeIcon />}
						onClick={() => wrap(() => unsnooze.call({ issue_id: issue.id }))}
					>
						Unsnooze
					</Button>
				) : (
					<Button
						size="small"
						variant="outlined"
						startIcon={<SnoozeIcon />}
						onClick={() => setSnoozeOpen((v) => !v)}
					>
						Snooze…
					</Button>
				)}
				<AddNoteButton
					apiModule="issues"
					parentKey="issue_id"
					parentId={issue.id}
					onAdded={onNoteAdded}
					variant="outlined"
				/>
			</Stack>
			{resolveOpen && (
				<Stack direction="row" spacing={1} sx={{ mt: 1, alignItems: "center" }}>
					<TextField
						select
						size="small"
						label="Reason"
						value={reason}
						onChange={(e) => setReason(e.target.value as ResolvedReason)}
						sx={{ minWidth: 160 }}
					>
						{RESOLVED_REASONS.map((r) => (
							<MenuItem key={r} value={r}>
								{RESOLVED_REASON_LABEL[r]}
							</MenuItem>
						))}
					</TextField>
					<Button
						variant="outlined"
						size="small"
						color="success"
						startIcon={<CheckCircleOutlinedIcon />}
						onClick={() =>
							wrap(() => resolve.call({ issue_id: issue.id, reason })).then(
								() => setResolveOpen(false),
							)
						}
					>
						Resolve
					</Button>
					<Button
						variant="outlined"
						size="small"
						onClick={() => setResolveOpen(false)}
					>
						Cancel
					</Button>
				</Stack>
			)}
			{snoozeOpen && (
				<Stack direction="row" spacing={1} sx={{ mt: 1, alignItems: "center" }}>
					<TextField
						type="number"
						size="small"
						label="Hours"
						value={snoozeHours}
						onChange={(e) => setSnoozeHours(Number(e.target.value))}
						sx={{ width: 100 }}
						slotProps={{ htmlInput: { min: 1, max: 24 * 30 } }}
					/>
					<Button
						variant="outlined"
						size="small"
						startIcon={<SnoozeIcon />}
						onClick={() => {
							const until = new Date(
								Date.now() + snoozeHours * 3_600_000,
							).toISOString();
							wrap(() => snooze.call({ issue_id: issue.id, until })).then(() =>
								setSnoozeOpen(false),
							);
						}}
					>
						Snooze
					</Button>
					<Button
						variant="outlined"
						size="small"
						onClick={() => setSnoozeOpen(false)}
					>
						Cancel
					</Button>
				</Stack>
			)}
			{error && (
				<MuiAlert severity="error" sx={{ mt: 1 }}>
					{error.message}
				</MuiAlert>
			)}
		</Box>
	);
}

/** One small chip per attaching incident, each linking to its detail page.
 * Open incidents are warning-coloured with a warning icon; closed ones use a
 * history icon and a neutral tone. Without these, there's no way to navigate
 * from an issue back to the incident(s) it joined. */
function IncidentLinks({ incidents }: { incidents: IssueIncidentLink[] }) {
	if (incidents.length === 0) return null;
	return (
		<Stack direction="row" spacing={0.5} sx={{ flexShrink: 0, flexWrap: "wrap" }}>
			{incidents.map((inc) => {
				const open = inc.closed_at == null;
				const opened = new Date(inc.opened_at).toLocaleString();
				const tooltip = open
					? `Open incident, opened ${opened}`
					: `Closed incident, opened ${opened}`;
				return (
					<Tooltip key={inc.incident_id} title={tooltip}>
						<Chip
							component={RouterLink}
							to={`/incidents/${inc.incident_id}`}
							size="small"
							variant="outlined"
							color={open ? "error" : "default"}
							icon={open ? <WarningAmberIcon /> : <HistoryIcon />}
							label="incident"
							clickable
						/>
					</Tooltip>
				);
			})}
		</Stack>
	);
}

function EventLog({ issueId }: { issueId: string }) {
	const result = useApi(
		"issues",
		"list_events",
		{ issue_id: issueId },
		[issueId],
	);

	if (result.status === "loading" || result.status === "idle")
		return (
			<Typography variant="caption" color="text.secondary">
				Loading…
			</Typography>
		);
	if (result.status === "error")
		return <MuiAlert severity="error">{result.error.message}</MuiAlert>;
	if (result.data.length === 0)
		return (
			<Typography variant="caption" color="text.secondary">
				No events recorded.
			</Typography>
		);

	return (
		<Stack spacing={0.5}>
			{result.data.map((e) => (
				<Box
					key={e.id}
					sx={{
						p: 1,
						border: 1,
						borderColor: "divider",
						borderRadius: 1,
					}}
				>
					<Stack
						direction="row"
						spacing={1}
						sx={{ alignItems: "center", minWidth: 0 }}
					>
						<Typography
							variant="body2"
							component="span"
							sx={{
								fontFamily: "monospace",
								fontSize: "0.85em",
								flex: 1,
								minWidth: 0,
								overflow: "hidden",
								textOverflow: "ellipsis",
								whiteSpace: "nowrap",
							}}
							title={e.message}
						>
							{e.message}
						</Typography>
						{e.occurrences > 1 && (
							<Typography
								variant="caption"
								color="text.secondary"
								sx={{ flexShrink: 0 }}
							>
								×{e.occurrences}
							</Typography>
						)}
						<Box sx={{ flexShrink: 0 }}>
							<SeverityChip severity={e.severity} />
						</Box>
						<Typography
							variant="caption"
							color="text.secondary"
							sx={{ flexShrink: 0 }}
						>
							<TimeAgo timestamp={e.occurred_at ?? e.created_at} />
						</Typography>
					</Stack>
				</Box>
			))}
		</Stack>
	);
}
