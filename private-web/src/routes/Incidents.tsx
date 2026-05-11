import {
	Alert as MuiAlert,
	Box,
	Checkbox,
	FormControlLabel,
	IconButton,
	LinearProgress,
	Link as MuiLink,
	ListItemText,
	MenuItem,
	Paper,
	Select,
	Stack,
	Switch,
	TextField,
	Typography,
} from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";
import { useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import SeverityChip from "../components/SeverityChip";
import TimeAgo from "../components/TimeAgo";
import { usePageTitle } from "../hooks/usePageTitle";
import {
	RESOLVED_REASONS,
	RESOLVED_REASON_LABEL,
	SEVERITIES,
	type IncidentData,
	type IssueData,
	type ResolvedReason,
	type ServerInfoFull,
	type Severity,
} from "../types";

const DEFAULT_LIMIT = 200;

type AckedFilter = "either" | "acked" | "unacked";

export default function Incidents() {
	usePageTitle("Incidents");

	// Filters live at page level. A single refresh tick refetches both the
	// incidents and issues queries together after any mutation.
	const [refreshTick, setRefreshTick] = useState(0);
	const bumpRefresh = () => setRefreshTick((t) => t + 1);

	const [activeOnly, setActiveOnly] = useState(true);
	const [severities, setSeverities] = useState<Severity[]>([]);
	const [groupId, setGroupId] = useState<string>(""); // "" = all
	const [acked, setAcked] = useState<AckedFilter>("either");

	const incidents = useApi<IncidentData[]>(
		"incidents",
		"list_active",
		{},
		[refreshTick],
	);
	const roots = useApi<ServerInfoFull[]>("servers", "list_roots", {}, []);
	const issues = useApi<IssueData[]>(
		"issues",
		"list",
		{
			activeOnly,
			severities: severities.length === 0 ? null : severities,
			serverGroupId: groupId === "" ? null : groupId,
			acked: acked === "either" ? null : acked === "acked",
			limit: DEFAULT_LIMIT,
		},
		[refreshTick, activeOnly, severities.join(","), groupId, acked],
	);

	return (
		<Stack spacing={3}>
			<Typography variant="h4" component="h1">
				Incidents
			</Typography>
			<OpenIncidents result={incidents} onChanged={bumpRefresh} />
			<FilterBar
				activeOnly={activeOnly}
				setActiveOnly={setActiveOnly}
				severities={severities}
				setSeverities={setSeverities}
				groupId={groupId}
				setGroupId={setGroupId}
				acked={acked}
				setAcked={setAcked}
				roots={roots}
				onRefresh={bumpRefresh}
			/>
			<IssuesList result={issues} onChanged={bumpRefresh} />
		</Stack>
	);
}

/** Issues and incidents already ship `server_name` + `server_host` on the
 * wire (the API joins servers). This helper is the rendering preference. */
function serverLabel(name: string | null, host: string): string {
	if (name && name.trim() !== "") return name;
	if (host && host.trim() !== "") return host;
	return "(unknown)";
}

function OpenIncidents({
	result,
	onChanged,
}: {
	result: ReturnType<typeof useApi<IncidentData[]>>;
	onChanged: () => void;
}) {
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
			>
				<Typography variant="h5" component="h2">
					Open incidents
					{result.status === "ok" && result.data.length > 0
						? ` (${result.data.length})`
						: ""}
				</Typography>
				<IconButton aria-label="Refresh" size="small" onClick={onChanged}>
					<RefreshIcon fontSize="small" />
				</IconButton>
			</Stack>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<MuiAlert severity="error">{result.error.message}</MuiAlert>
			) : result.data.length === 0 ? (
				<MuiAlert severity="success">No open incidents.</MuiAlert>
			) : (
				<Stack spacing={1}>
					{result.data.map((inc) => (
						<IncidentRow
							key={inc.id}
							incident={inc}
							groupName={serverLabel(inc.server_name, inc.server_host)}
							onChanged={onChanged}
						/>
					))}
				</Stack>
			)}
		</Paper>
	);
}

function IncidentRow({
	incident,
	groupName,
	onChanged,
}: {
	incident: IncidentData;
	groupName: string;
	onChanged: () => void;
}) {
	const ack = useApiAction("incidents", "ack");
	const unack = useApiAction("incidents", "unack");
	const resolve = useApiAction("incidents", "resolve");
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
	const error = ack.error ?? unack.error ?? resolve.error;

	return (
		<Box
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "error.main",
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
						bgcolor: "error.main",
						flexShrink: 0,
					}}
				/>
				<MuiLink
					component={RouterLink}
					to={`/servers/${incident.server_id}`}
					underline="hover"
					color="text.primary"
					sx={{ fontWeight: 500 }}
				>
					{groupName}
				</MuiLink>
				<Typography variant="body2" color="text.secondary">
					opened <TimeAgo timestamp={incident.opened_at} />
				</Typography>
				{incident.acknowledged_at && (
					<Typography
						variant="caption"
						color="info.main"
						title={`by ${incident.acknowledged_by ?? "?"}`}
					>
						acked
					</Typography>
				)}
				{incident.resolved_at && (
					<Typography
						variant="caption"
						color="success.main"
						title={`(${incident.resolved_reason ?? "?"}) by ${incident.resolved_by ?? "?"}`}
					>
						resolved
					</Typography>
				)}
			</Stack>
			<Stack direction="row" spacing={1} sx={{ mt: 1, flexWrap: "wrap" }} useFlexGap>
				{incident.acknowledged_at ? (
					<MuiLink
						component="button"
						onClick={() => wrap(() => unack.call({ incident_id: incident.id }))}
					>
						Unack
					</MuiLink>
				) : (
					<MuiLink
						component="button"
						onClick={() => wrap(() => ack.call({ incident_id: incident.id }))}
					>
						Ack
					</MuiLink>
				)}
				{!incident.resolved_at && (
					<MuiLink
						component="button"
						color="success.main"
						onClick={() => setResolveOpen((v) => !v)}
					>
						Resolve…
					</MuiLink>
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
					<MuiLink
						component="button"
						onClick={() =>
							wrap(() => resolve.call({ incident_id: incident.id, reason })).then(() =>
								setResolveOpen(false),
							)
						}
					>
						Resolve
					</MuiLink>
					<MuiLink component="button" onClick={() => setResolveOpen(false)}>
						Cancel
					</MuiLink>
				</Stack>
			)}
			{error && <MuiAlert severity="error" sx={{ mt: 1 }}>{error.message}</MuiAlert>}
		</Box>
	);
}

function FilterBar({
	activeOnly,
	setActiveOnly,
	severities,
	setSeverities,
	groupId,
	setGroupId,
	acked,
	setAcked,
	roots,
	onRefresh,
}: {
	activeOnly: boolean;
	setActiveOnly: (v: boolean) => void;
	severities: Severity[];
	setSeverities: (v: Severity[]) => void;
	groupId: string;
	setGroupId: (v: string) => void;
	acked: AckedFilter;
	setAcked: (v: AckedFilter) => void;
	roots: ReturnType<typeof useApi<ServerInfoFull[]>>;
	onRefresh: () => void;
}) {
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<FormControlLabel
					control={
						<Switch
							size="small"
							checked={!activeOnly}
							onChange={(e) => setActiveOnly(!e.target.checked)}
						/>
					}
					label="Show all"
				/>
				<TextField
					select
					size="small"
					label="Severities"
					value={severities}
					onChange={(e) => {
						const v = e.target.value;
						setSeverities(typeof v === "string" ? (v.split(",") as Severity[]) : (v as Severity[]));
					}}
					slotProps={{
						select: {
							multiple: true,
							renderValue: (selected) =>
								(selected as Severity[]).length === 0
									? "All"
									: (selected as Severity[]).join(", "),
						},
					}}
					sx={{ minWidth: 200 }}
				>
					{SEVERITIES.map((s) => (
						<MenuItem key={s} value={s}>
							<Checkbox checked={severities.includes(s)} size="small" />
							<ListItemText primary={s} />
						</MenuItem>
					))}
				</TextField>
				<TextField
					select
					size="small"
					label="Server group"
					value={groupId}
					onChange={(e) => setGroupId(e.target.value)}
					sx={{ minWidth: 200 }}
				>
					<MenuItem value="">All groups</MenuItem>
					{roots.status === "ok" &&
						roots.data.map((r) => (
							<MenuItem key={r.id} value={r.id}>
								{r.name ?? r.id}
							</MenuItem>
						))}
				</TextField>
				<Select
					size="small"
					value={acked}
					onChange={(e) => setAcked(e.target.value as AckedFilter)}
					sx={{ minWidth: 140 }}
				>
					<MenuItem value="either">Acked: any</MenuItem>
					<MenuItem value="acked">Acked only</MenuItem>
					<MenuItem value="unacked">Unacked only</MenuItem>
				</Select>
				<Box sx={{ ml: "auto" }}>
					<IconButton aria-label="Refresh" size="small" onClick={onRefresh}>
						<RefreshIcon fontSize="small" />
					</IconButton>
				</Box>
			</Stack>
		</Paper>
	);
}

function IssuesList({
	result,
	onChanged,
}: {
	result: ReturnType<typeof useApi<IssueData[]>>;
	onChanged: () => void;
}) {
	if (result.status === "loading" || result.status === "idle") {
		return <LinearProgress />;
	}
	if (result.status === "error") {
		return <MuiAlert severity="error">{result.error.message}</MuiAlert>;
	}
	if (result.data.length === 0) {
		return <MuiAlert severity="success">No issues match the current filters.</MuiAlert>;
	}
	return (
		<Stack spacing={1}>
			{result.data.map((issue) => (
				<IssueRow
					key={issue.id}
					issue={issue}
					serverName={serverLabel(issue.server_name, issue.server_host)}
					onChanged={onChanged}
				/>
			))}
		</Stack>
	);
}

function IssueRow({
	issue,
	serverName,
	onChanged,
}: {
	issue: IssueData;
	serverName: string;
	onChanged: () => void;
}) {
	const ack = useApiAction("issues", "ack");
	const unack = useApiAction("issues", "unack");
	const wrap = async (fn: () => Promise<unknown>) => {
		try {
			await fn();
			onChanged();
		} catch {
			/* surfaced via *.error */
		}
	};
	return (
		<Box
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
				bgcolor: issue.active ? undefined : "action.hover",
			}}
		>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<SeverityChip severity={issue.severity} />
				<MuiLink
					component={RouterLink}
					to={`/servers/${issue.server_id}`}
					underline="hover"
					color="text.primary"
					sx={{ fontWeight: 500 }}
				>
					{serverName}
				</MuiLink>
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
					<Typography variant="caption" color="success.main">
						resolved
					</Typography>
				)}
				<Box sx={{ ml: "auto" }}>
					<Typography variant="body2" color="text.secondary">
						<TimeAgo timestamp={issue.last_seen} />
					</Typography>
				</Box>
				{issue.acknowledged_at ? (
					<MuiLink
						component="button"
						onClick={() => wrap(() => unack.call({ issue_id: issue.id }))}
					>
						Unack
					</MuiLink>
				) : (
					<MuiLink
						component="button"
						onClick={() => wrap(() => ack.call({ issue_id: issue.id }))}
					>
						Ack
					</MuiLink>
				)}
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
		</Box>
	);
}
