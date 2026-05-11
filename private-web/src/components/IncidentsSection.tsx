import {
	Alert as MuiAlert,
	Box,
	Button,
	Collapse,
	FormControlLabel,
	IconButton,
	LinearProgress,
	MenuItem,
	Paper,
	Stack,
	Switch,
	TextField,
	Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import RefreshIcon from "@mui/icons-material/Refresh";
import { useState } from "react";
import { useApi, useApiAction } from "../api";
import NotesPanel from "./NotesPanel";
import TimeAgo from "./TimeAgo";
import {
	RESOLVED_REASONS,
	RESOLVED_REASON_LABEL,
	type IncidentData,
	type ResolvedReason,
} from "../types";

export default function IncidentsSection({
	serverId,
	refreshKey = 0,
	onChanged,
}: {
	serverId: string;
	/** Bump to force a refetch. */
	refreshKey?: number;
	/** Called on any mutation. Falls back to local reload when unset. */
	onChanged?: () => void;
}) {
	const [showAll, setShowAll] = useState(false);
	const result = useApi<IncidentData[]>(
		"incidents",
		"list_for_server",
		{ server_id: serverId, include_closed: showAll },
		[serverId, showAll, refreshKey],
	);
	const notify = onChanged ?? result.reload;

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="h6" component="h3">
					Incidents
				</Typography>
				<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
					<FormControlLabel
						control={
							<Switch
								size="small"
								checked={showAll}
								onChange={(e) => setShowAll(e.target.checked)}
							/>
						}
						label="Show all"
					/>
					<IconButton
						aria-label="Refresh incidents"
						size="small"
						onClick={notify}
					>
						<RefreshIcon fontSize="small" />
					</IconButton>
				</Stack>
			</Stack>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<MuiAlert severity="error">{result.error.message}</MuiAlert>
			) : result.data.length === 0 ? (
				<MuiAlert severity="success">
					No {showAll ? "" : "open "}incidents.
				</MuiAlert>
			) : (
				<Stack spacing={1}>
					{result.data.map((inc) => (
						<IncidentRow
							key={inc.id}
							incident={inc}
							onChanged={notify}
						/>
					))}
				</Stack>
			)}
		</Paper>
	);
}

function IncidentRow({
	incident,
	onChanged,
}: {
	incident: IncidentData;
	onChanged: () => void;
}) {
	const open = incident.closed_at == null;
	const ack = useApiAction("incidents", "ack");
	const unack = useApiAction("incidents", "unack");
	const resolve = useApiAction("incidents", "resolve");
	const unresolve = useApiAction("incidents", "unresolve");

	const [expanded, setExpanded] = useState(false);
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
	const error = ack.error ?? unack.error ?? resolve.error ?? unresolve.error;

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
				<Box
					sx={{
						width: 10,
						height: 10,
						borderRadius: "50%",
						bgcolor: open ? "error.main" : "text.disabled",
						flexShrink: 0,
					}}
				/>
				<Typography variant="body2">
					{open ? "Open" : "Closed"} — opened <TimeAgo timestamp={incident.opened_at} />
					{!open && incident.closed_at && (
						<>
							, closed <TimeAgo timestamp={incident.closed_at} />
						</>
					)}
				</Typography>
				{incident.acknowledged_at && (
					<Typography variant="caption" color="info.main" title={`by ${incident.acknowledged_by ?? "?"}`}>
						acked
					</Typography>
				)}
				{incident.resolved_at && (
					<Typography variant="caption" color="success.main" title={`(${incident.resolved_reason ?? "?"}) by ${incident.resolved_by ?? "?"}`}>
						resolved
					</Typography>
				)}
				<Typography
					variant="caption"
					color="text.secondary"
					sx={{ ml: "auto", fontFamily: "monospace" }}
				>
					{incident.id}
				</Typography>
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
			</Stack>
			<Stack direction="row" spacing={1} sx={{ mt: 1, flexWrap: "wrap" }} useFlexGap>
				{incident.acknowledged_at ? (
					<Button
						size="small"
						onClick={() => wrap(() => unack.call({ incident_id: incident.id }))}
					>
						Unack
					</Button>
				) : (
					<Button
						size="small"
						onClick={() => wrap(() => ack.call({ incident_id: incident.id }))}
					>
						Ack
					</Button>
				)}
				{incident.resolved_at ? (
					<Button
						size="small"
						color="warning"
						onClick={() => wrap(() => unresolve.call({ incident_id: incident.id }))}
					>
						Unresolve
					</Button>
				) : (
					<Button size="small" color="success" onClick={() => setResolveOpen((v) => !v)}>
						Resolve…
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
							wrap(() => resolve.call({ incident_id: incident.id, reason })).then(() =>
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
			{error && (
				<MuiAlert severity="error" sx={{ mt: 1 }}>
					{error.message}
				</MuiAlert>
			)}
			<Collapse in={expanded} unmountOnExit>
				<Box sx={{ mt: 1 }}>
					<Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 0.5 }}>
						Notes
					</Typography>
					<NotesPanel apiModule="incidents" parentKey="incident_id" parentId={incident.id} />
				</Box>
			</Collapse>
		</Box>
	);
}
