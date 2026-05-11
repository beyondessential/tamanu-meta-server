import {
	Alert as MuiAlert,
	Box,
	Button,
	Chip,
	IconButton,
	LinearProgress,
	Link as MuiLink,
	MenuItem,
	Paper,
	Stack,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import BugReportIcon from "@mui/icons-material/BugReport";
import NotesIcon from "@mui/icons-material/StickyNote2";
import RefreshIcon from "@mui/icons-material/Refresh";
import TimelineIcon from "@mui/icons-material/Timeline";
import { useState } from "react";
import { Link as RouterLink, useParams } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import IssueRow from "../components/IssueRow";
import ManualEventButton from "../components/ManualEventButton";
import { AddNoteButton } from "../components/NotesList";
import TimeAgo from "../components/TimeAgo";
import UserAvatar from "../components/UserAvatar";
import { usePageTitle } from "../hooks/usePageTitle";
import { humanDuration } from "../lib/humanDuration";
import {
	RESOLVED_REASONS,
	RESOLVED_REASON_LABEL,
	type IncidentIssueData,
	type IncidentNoteData,
	type IncidentWithIssues,
	type ResolvedReason,
} from "../types";

type Filter = "all" | "issues" | "notes";

function serverLabel(name: string | null, host: string): string {
	if (name && name.trim() !== "") return name;
	if (host && host.trim() !== "") return host;
	return "(unknown)";
}

export default function IncidentDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const [refreshTick, setRefreshTick] = useState(0);
	const bumpRefresh = () => setRefreshTick((t) => t + 1);
	const [filter, setFilter] = useState<Filter>("all");

	const detail = useApi<IncidentWithIssues>(
		"incidents",
		"get",
		{ incident_id: id },
		[id, refreshTick],
	);
	const notes = useApi<IncidentNoteData[]>(
		"incidents",
		"list_notes",
		{ incident_id: id },
		[id, refreshTick],
	);

	usePageTitle(
		detail.status === "ok"
			? `Incident on ${serverLabel(detail.data.incident.server_name, detail.data.incident.server_host)}`
			: "Incident",
	);

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <MuiAlert severity="error">{detail.error.message}</MuiAlert>;
	}

	const { incident, issues } = detail.data;
	const noteList = notes.status === "ok" ? notes.data : [];

	return (
		<Stack spacing={3}>
			<Header incident={incident} onChanged={bumpRefresh} />

			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
			>
				<Chip
					label="All"
					color={filter === "all" ? "primary" : "default"}
					onClick={() => setFilter("all")}
					clickable
				/>
				<Chip
					icon={<BugReportIcon />}
					label={`Issues (${issues.length})`}
					color={filter === "issues" ? "primary" : "default"}
					onClick={() => setFilter("issues")}
					clickable
				/>
				<Chip
					icon={<NotesIcon />}
					label={`Notes (${noteList.length})`}
					color={filter === "notes" ? "primary" : "default"}
					onClick={() => setFilter("notes")}
					clickable
				/>
				<Box sx={{ ml: "auto" }}>
					<IconButton aria-label="Refresh" size="small" onClick={bumpRefresh}>
						<RefreshIcon fontSize="small" />
					</IconButton>
				</Box>
			</Stack>

			<Timeline
				issues={filter === "notes" ? [] : issues}
				notes={filter === "issues" ? [] : noteList}
				onChanged={bumpRefresh}
			/>
		</Stack>
	);
}

function Header({
	incident,
	onChanged,
}: {
	incident: IncidentWithIssues["incident"];
	onChanged: () => void;
}) {
	const open = incident.closed_at == null;
	const ack = useApiAction("incidents", "ack");
	const resolve = useApiAction("incidents", "resolve");
	const unresolve = useApiAction("incidents", "unresolve");

	const [resolveOpen, setResolveOpen] = useState(false);
	const [reason, setReason] = useState<ResolvedReason>("fixed");

	const wrap = async (fn: () => Promise<unknown>) => {
		try {
			await fn();
			onChanged();
		} catch {
			/* surfaced via *.error */
		}
	};
	const error = ack.error ?? resolve.error ?? unresolve.error;

	const timeText = (() => {
		if (open) {
			return (
				<>
					opened <TimeAgo timestamp={incident.opened_at} />
				</>
			);
		}
		const closedAt = incident.closed_at;
		if (!closedAt) return null;
		const lasted = humanDuration(incident.opened_at, closedAt);
		return (
			<>
				closed <TimeAgo timestamp={closedAt} />, lasted {lasted}
			</>
		);
	})();

	return (
		<Paper
			variant="outlined"
			sx={{
				p: 2,
				borderColor: open ? "error.main" : "divider",
				borderWidth: open ? 2 : 1,
			}}
		>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "flex-start", justifyContent: "space-between" }}
			>
				<Box sx={{ minWidth: 0, flex: 1 }}>
					<Typography variant="h4" component="h1">
						Incident on{" "}
						<MuiLink
							component={RouterLink}
							to={`/servers/${incident.server_id}`}
							underline="hover"
							color="inherit"
						>
							{serverLabel(incident.server_name, incident.server_host)}
						</MuiLink>
					</Typography>
					<Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
						{timeText}
					</Typography>
				</Box>
				{incident.acknowledged_at && (
					<Box sx={{ flexShrink: 0 }}>
						<UserAvatar
							login={incident.acknowledged_by}
							name={incident.acknowledged_by_name}
							profilePic={incident.acknowledged_by_pic}
							size={36}
						/>
					</Box>
				)}
			</Stack>

			<Stack
				direction="row"
				spacing={1}
				sx={{ mt: 2, alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				{!incident.acknowledged_at && (
					<Button
						size="small"
						variant="outlined"
						onClick={() => wrap(() => ack.call({ incident_id: incident.id }))}
					>
						Ack
					</Button>
				)}
				{incident.resolved_at ? (
					<Button
						size="small"
						variant="outlined"
						color="warning"
						onClick={() =>
							wrap(() => unresolve.call({ incident_id: incident.id }))
						}
					>
						Unresolve
					</Button>
				) : (
					<Tooltip
						title={incident.acknowledged_at ? "" : "Ack the incident first"}
						disableHoverListener={!!incident.acknowledged_at}
					>
						<span>
							<Button
								size="small"
								variant="outlined"
								color="success"
								disabled={!incident.acknowledged_at}
								onClick={() => setResolveOpen((v) => !v)}
							>
								Resolve…
							</Button>
						</span>
					</Tooltip>
				)}
				<ManualEventButton
					serverId={incident.server_id}
					hasOpenIncident={open}
					onSubmitted={onChanged}
					size="small"
				/>
				<AddNoteButton
					apiModule="incidents"
					parentKey="incident_id"
					parentId={incident.id}
					onAdded={onChanged}
					variant="outlined"
				/>
				<Box sx={{ ml: "auto" }}>
					<Stack direction="row" spacing={2} sx={{ alignItems: "center" }}>
						<Stat
							icon={<BugReportIcon fontSize="inherit" />}
							value={incident.issue_count ?? 0}
							noun="issue"
						/>
						<Stat
							icon={<TimelineIcon fontSize="inherit" />}
							value={incident.event_count ?? 0}
							noun="event"
						/>
						<Stat
							icon={<NotesIcon fontSize="inherit" />}
							value={incident.note_count ?? 0}
							noun="note"
						/>
					</Stack>
				</Box>
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
						onClick={() =>
							wrap(() =>
								resolve.call({ incident_id: incident.id, reason }),
							).then(() => setResolveOpen(false))
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
			{error && (
				<MuiAlert severity="error" sx={{ mt: 1 }}>
					{error.message}
				</MuiAlert>
			)}
		</Paper>
	);
}

type TimelineEntry =
	| { kind: "issue"; at: number; issue: IncidentIssueData }
	| { kind: "note"; at: number; note: IncidentNoteData };

function Timeline({
	issues,
	notes,
	onChanged,
}: {
	issues: IncidentIssueData[];
	notes: IncidentNoteData[];
	onChanged: () => void;
}) {
	const entries: TimelineEntry[] = [
		...issues.map<TimelineEntry>((i) => ({
			kind: "issue",
			at: Date.parse(i.joined_at),
			issue: i,
		})),
		...notes.map<TimelineEntry>((n) => ({
			kind: "note",
			at: Date.parse(n.created_at),
			note: n,
		})),
	];
	entries.sort((a, b) => b.at - a.at);

	if (entries.length === 0) {
		return <MuiAlert severity="info">No timeline entries yet.</MuiAlert>;
	}

	return (
		<Stack spacing={1}>
			{entries.map((e) =>
				e.kind === "issue" ? (
					<IssueRow
						key={`issue-${e.issue.issue.id}`}
						issue={e.issue.issue}
						defaultExpanded
						onChanged={onChanged}
					/>
				) : (
					<NoteEntry key={`note-${e.note.id}`} note={e.note} />
				),
			)}
		</Stack>
	);
}

function NoteEntry({ note }: { note: IncidentNoteData }) {
	return (
		<Box
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
				bgcolor: "background.paper",
			}}
		>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
					<NotesIcon fontSize="small" color="action" />
					<Typography variant="caption" color="text.secondary">
						{note.author}
					</Typography>
				</Stack>
				<Typography variant="caption" color="text.secondary">
					<TimeAgo timestamp={note.created_at} />
				</Typography>
			</Stack>
			<Typography
				variant="body2"
				component="pre"
				sx={{ mt: 0.5, mb: 0, whiteSpace: "pre-wrap", fontFamily: "inherit" }}
			>
				{note.body}
			</Typography>
		</Box>
	);
}

function Stat({
	icon,
	value,
	noun,
}: {
	icon: React.ReactNode;
	value: number;
	noun: string;
}) {
	const title = `${value} ${noun}${value === 1 ? "" : "s"}`;
	return (
		<Tooltip title={title}>
			<Stack
				direction="row"
				spacing={0.5}
				sx={{
					alignItems: "center",
					color: "text.secondary",
					fontSize: "0.875rem",
				}}
			>
				{icon}
				<Box component="span">{value}</Box>
			</Stack>
		</Tooltip>
	);
}
