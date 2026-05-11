import {
	Alert as MuiAlert,
	Box,
	Button,
	Collapse,
	IconButton,
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
import { useApiAction } from "../api";
import NotesList, { AddNoteButton } from "./NotesList";
import TimeAgo from "./TimeAgo";
import UserAvatar from "./UserAvatar";
import { humanDuration } from "../lib/humanDuration";
import {
	RESOLVED_REASONS,
	RESOLVED_REASON_LABEL,
	type IncidentData,
	type ResolvedReason,
} from "../types";

function serverLabel(name: string | null, host: string): string {
	if (name && name.trim() !== "") return name;
	if (host && host.trim() !== "") return host;
	return "(unknown)";
}

/** One incident with action controls (Ack/Resolve — no Unack in UI) and a
 * collapsible notes panel. Used by `IncidentsSection` on a server's page.
 * `showServer` toggles a server-group link at the start of the row. */
export default function IncidentRow({
	incident,
	showServer = false,
	onChanged,
}: {
	incident: IncidentData;
	showServer?: boolean;
	onChanged: () => void;
}) {
	const open = incident.closed_at == null;
	const ack = useApiAction("incidents", "ack");
	const resolve = useApiAction("incidents", "resolve");
	const unresolve = useApiAction("incidents", "unresolve");

	const [expanded, setExpanded] = useState(false);
	const [resolveOpen, setResolveOpen] = useState(false);
	const [reason, setReason] = useState<ResolvedReason>("fixed");
	const [notesRefresh, setNotesRefresh] = useState(0);

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
		<Box
			sx={{
				p: 1.5,
				border: 1,
				borderColor: open ? "error.main" : "divider",
				borderRadius: 1,
			}}
		>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<IconButton
					aria-label={expanded ? "Hide notes" : "Show notes"}
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
						to={`/servers/${incident.server_id}`}
						underline="hover"
						color="text.primary"
						sx={{ fontWeight: 500 }}
					>
						{serverLabel(incident.server_name, incident.server_host)}
					</MuiLink>
				)}
				<Typography variant="body2" sx={{ flex: 1 }}>
					{timeText}
				</Typography>
				{incident.resolved_at ? (
					<Tooltip
						title={`resolved (${incident.resolved_reason ?? "?"}) by ${
							incident.resolved_by_name ?? incident.resolved_by ?? "?"
						}`}
					>
						<span>
							<UserAvatar
								login={incident.resolved_by}
								name={incident.resolved_by_name}
								profilePic={incident.resolved_by_pic}
							/>
						</span>
					</Tooltip>
				) : incident.acknowledged_at ? (
					<Tooltip
						title={`acked by ${incident.acknowledged_by_name ?? incident.acknowledged_by ?? "?"}`}
					>
						<span>
							<UserAvatar
								login={incident.acknowledged_by}
								name={incident.acknowledged_by_name}
								profilePic={incident.acknowledged_by_pic}
							/>
						</span>
					</Tooltip>
				) : (
					<Button
						size="small"
						onClick={() => wrap(() => ack.call({ incident_id: incident.id }))}
						disabled={ack.pending}
					>
						Ack
					</Button>
				)}
			</Stack>
			<Stack direction="row" spacing={1} sx={{ mt: 1, flexWrap: "wrap" }} useFlexGap>
				{incident.resolved_at ? (
					<Button
						size="small"
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
								color="success"
								disabled={!incident.acknowledged_at}
								onClick={() => setResolveOpen((v) => !v)}
							>
								Resolve…
							</Button>
						</span>
					</Tooltip>
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
							wrap(() =>
								resolve.call({ incident_id: incident.id, reason }),
							).then(() => setResolveOpen(false))
						}
					>
						Resolve
					</Button>
					<Button size="small" onClick={() => setResolveOpen(false)}>
						Cancel
					</Button>
				</Stack>
			)}
			{error && (
				<MuiAlert severity="error" sx={{ mt: 1 }}>
					{error.message}
				</MuiAlert>
			)}
			<Collapse in={expanded} unmountOnExit>
				<Box sx={{ mt: 1 }}>
					<Stack direction="row" sx={{ justifyContent: "flex-end", mb: 1 }}>
						<AddNoteButton
							apiModule="incidents"
							parentKey="incident_id"
							parentId={incident.id}
							onAdded={() => setNotesRefresh((t) => t + 1)}
						/>
					</Stack>
					<NotesList
						apiModule="incidents"
						parentKey="incident_id"
						parentId={incident.id}
						refreshKey={notesRefresh}
					/>
				</Box>
			</Collapse>
		</Box>
	);
}
