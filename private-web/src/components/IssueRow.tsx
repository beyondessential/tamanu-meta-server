import {
	Alert as MuiAlert,
	Box,
	Button,
	Collapse,
	IconButton,
	LinearProgress,
	Link as MuiLink,
	MenuItem,
	Stack,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import { useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import NotesPanel from "./NotesPanel";
import SeverityChip from "./SeverityChip";
import TimeAgo from "./TimeAgo";
import {
	RESOLVED_REASONS,
	RESOLVED_REASON_LABEL,
	type EventData,
	type IssueData,
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

/** One issue, with its full controls: Ack/Resolve/Snooze, expandable event log
 * and notes panel. Used by `IssuesSection` on detail pages and by the global
 * Incidents page. `showServer` toggles a "go to server" link at the start of
 * the row — useful on the global page where rows mix servers, redundant on a
 * single-server detail page where the server is already in context. */
export default function IssueRow({
	issue,
	showServer = false,
	onChanged,
}: {
	issue: IssueData;
	showServer?: boolean;
	onChanged: () => void;
}) {
	const [expanded, setExpanded] = useState(false);
	const snoozeActive = isSnoozeActive(issue.snoozed_until);
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
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<SeverityChip severity={issue.severity} />
				{showServer && (
					<MuiLink
						component={RouterLink}
						to={`/servers/${issue.server_id}`}
						underline="hover"
						color="text.primary"
						sx={{ fontWeight: 500 }}
					>
						{serverLabel(issue.server_name, issue.server_host)}
					</MuiLink>
				)}
				{!issue.active && (
					<Typography variant="caption" color="text.secondary">
						(inactive)
					</Typography>
				)}
				{issue.acknowledged_at && (
					<Typography
						variant="caption"
						color="info.main"
						title={`acked by ${issue.acknowledged_by ?? "?"}`}
					>
						acked
					</Typography>
				)}
				{issue.resolved_at && (
					<Typography
						variant="caption"
						color="success.main"
						title={`resolved (${issue.resolved_reason ?? "?"}) by ${issue.resolved_by ?? "?"}`}
					>
						resolved
					</Typography>
				)}
				{snoozeActive && (
					<Typography
						variant="caption"
						color="warning.main"
						title={`until ${issue.snoozed_until}`}
					>
						snoozed
					</Typography>
				)}
				<Typography
					variant="body2"
					sx={{ fontFamily: "monospace" }}
					color="text.secondary"
				>
					{issue.source}/{issue.ref}
				</Typography>
				{issue.description && (
					<Typography variant="subtitle2">{issue.description}</Typography>
				)}
				<Box sx={{ ml: "auto" }}>
					<Typography variant="body2" color="text.secondary">
						<TimeAgo timestamp={issue.last_seen} />
						{issue.first_seen !== issue.last_seen && (
							<>
								{" (first "}
								<TimeAgo timestamp={issue.first_seen} />)
							</>
						)}
					</Typography>
				</Box>
				<IconButton
					aria-label={expanded ? "Collapse" : "Show event log"}
					size="small"
					onClick={() => setExpanded((v) => !v)}
				>
					{expanded ? (
						<ExpandLessIcon fontSize="small" />
					) : (
						<ExpandMoreIcon fontSize="small" />
					)}
				</IconButton>
			</Stack>
			<Typography
				variant="body2"
				component="pre"
				sx={{
					mt: 1,
					mb: 0,
					whiteSpace: "pre-wrap",
					fontFamily: "monospace",
					fontSize: "0.85em",
				}}
			>
				{issue.message}
			</Typography>
			<IssueActions issue={issue} snoozeActive={snoozeActive} onChanged={onChanged} />
			<Collapse in={expanded} unmountOnExit>
				<Box sx={{ mt: 1 }}>
					<Typography
						variant="caption"
						color="text.secondary"
						sx={{ display: "block", mb: 0.5 }}
					>
						Events
					</Typography>
					<EventLog issueId={issue.id} />
					<Typography
						variant="caption"
						color="text.secondary"
						sx={{ display: "block", mt: 1.5, mb: 0.5 }}
					>
						Notes
					</Typography>
					<NotesPanel apiModule="issues" parentKey="issue_id" parentId={issue.id} />
				</Box>
			</Collapse>
		</Box>
	);
}

function IssueActions({
	issue,
	snoozeActive,
	onChanged,
}: {
	issue: IssueData;
	snoozeActive: boolean;
	onChanged: () => void;
}) {
	const ack = useApiAction("issues", "ack");
	const unack = useApiAction("issues", "unack");
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
		ack.error ?? unack.error ?? resolve.error ?? unresolve.error ?? snooze.error ?? unsnooze.error;

	return (
		<Box sx={{ mt: 1 }}>
			<Stack direction="row" spacing={1} sx={{ flexWrap: "wrap" }} useFlexGap>
				{issue.acknowledged_at ? (
					<Button
						size="small"
						onClick={() => wrap(() => unack.call({ issue_id: issue.id }))}
					>
						Unack
					</Button>
				) : (
					<Button
						size="small"
						onClick={() => wrap(() => ack.call({ issue_id: issue.id }))}
					>
						Ack
					</Button>
				)}
				{issue.resolved_at ? (
					<Button
						size="small"
						color="warning"
						onClick={() => wrap(() => unresolve.call({ issue_id: issue.id }))}
					>
						Unresolve
					</Button>
				) : (
					<Tooltip
						title={
							issue.acknowledged_at ? "" : "Ack the issue first"
						}
						disableHoverListener={!!issue.acknowledged_at}
					>
						{/* span so Tooltip can wrap a disabled Button */}
						<span>
							<Button
								size="small"
								color="success"
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
						color="warning"
						onClick={() => wrap(() => unsnooze.call({ issue_id: issue.id }))}
					>
						Unsnooze
					</Button>
				) : (
					<Button size="small" onClick={() => setSnoozeOpen((v) => !v)}>
						Snooze…
					</Button>
				)}
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
						variant="contained"
						size="small"
						onClick={() =>
							wrap(() => resolve.call({ issue_id: issue.id, reason })).then(() =>
								setResolveOpen(false),
							)
						}
					>
						Resolve
					</Button>
					<Button size="small" onClick={() => setResolveOpen(false)}>
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
						variant="contained"
						size="small"
						onClick={() => {
							const until = new Date(Date.now() + snoozeHours * 3_600_000).toISOString();
							wrap(() => snooze.call({ issue_id: issue.id, until })).then(() =>
								setSnoozeOpen(false),
							);
						}}
					>
						Snooze
					</Button>
					<Button size="small" onClick={() => setSnoozeOpen(false)}>
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

function EventLog({ issueId }: { issueId: string }) {
	const result = useApi<EventData[]>(
		"issues",
		"list_events",
		{ issue_id: issueId },
		[issueId],
	);

	if (result.status === "loading" || result.status === "idle") return <LinearProgress />;
	if (result.status === "error")
		return <MuiAlert severity="error">{result.error.message}</MuiAlert>;
	if (result.data.length === 0)
		return <MuiAlert severity="info">No events recorded.</MuiAlert>;

	return (
		<Stack spacing={0.5}>
			{result.data.map((e) => (
				<Stack
					key={e.id}
					direction="row"
					spacing={1}
					sx={{ alignItems: "center", flexWrap: "wrap", fontSize: "0.85em" }}
					useFlexGap
				>
					<SeverityChip severity={e.severity} />
					<Typography variant="caption" color="text.secondary">
						{e.active ? "active" : "resolved"}
					</Typography>
					<Typography
						variant="body2"
						component="span"
						sx={{ fontFamily: "monospace" }}
					>
						{e.message}
					</Typography>
					{e.occurrences > 1 && (
						<Typography variant="caption" color="text.secondary">
							×{e.occurrences}
						</Typography>
					)}
					<Box sx={{ ml: "auto" }}>
						<Typography variant="caption" color="text.secondary">
							<TimeAgo timestamp={e.occurred_at ?? e.created_at} />
						</Typography>
					</Box>
				</Stack>
			))}
		</Stack>
	);
}
