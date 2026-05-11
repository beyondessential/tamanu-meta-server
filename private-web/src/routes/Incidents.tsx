import {
	Alert as MuiAlert,
	Box,
	Checkbox,
	FormControlLabel,
	IconButton,
	LinearProgress,
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
import { useApi } from "../api";
import IncidentRow from "../components/IncidentRow";
import IssueRow from "../components/IssueRow";
import { usePageTitle } from "../hooks/usePageTitle";
import {
	SEVERITIES,
	type IncidentData,
	type IssueData,
	type ServerInfoFull,
	type Severity,
} from "../types";

const DEFAULT_LIMIT = 200;

type AckedFilter = "either" | "acked" | "unacked";

export default function Incidents() {
	usePageTitle("Incidents");

	// Page-level refresh signal: a single tick refetches incidents + issues
	// together after any mutation (ack, resolve, snooze, manual submit).
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

			<Paper variant="outlined" sx={{ p: 2 }}>
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center", justifyContent: "space-between", mb: 1 }}
				>
					<Typography variant="h5" component="h2">
						Open incidents
						{incidents.status === "ok" && incidents.data.length > 0
							? ` (${incidents.data.length})`
							: ""}
					</Typography>
					<IconButton aria-label="Refresh" size="small" onClick={bumpRefresh}>
						<RefreshIcon fontSize="small" />
					</IconButton>
				</Stack>
				{incidents.status === "loading" || incidents.status === "idle" ? (
					<LinearProgress />
				) : incidents.status === "error" ? (
					<MuiAlert severity="error">{incidents.error.message}</MuiAlert>
				) : incidents.data.length === 0 ? (
					<MuiAlert severity="success">No open incidents.</MuiAlert>
				) : (
					<Stack spacing={1}>
						{incidents.data.map((inc) => (
							<IncidentRow
								key={inc.id}
								incident={inc}
								showServer
								onChanged={bumpRefresh}
							/>
						))}
					</Stack>
				)}
			</Paper>

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

			{issues.status === "loading" || issues.status === "idle" ? (
				<LinearProgress />
			) : issues.status === "error" ? (
				<MuiAlert severity="error">{issues.error.message}</MuiAlert>
			) : issues.data.length === 0 ? (
				<MuiAlert severity="success">No issues match the current filters.</MuiAlert>
			) : (
				<Stack spacing={1}>
					{issues.data.map((issue) => (
						<IssueRow
							key={issue.id}
							issue={issue}
							showServer
							onChanged={bumpRefresh}
						/>
					))}
				</Stack>
			)}
		</Stack>
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
						setSeverities(
							typeof v === "string"
								? (v.split(",") as Severity[])
								: (v as Severity[]),
						);
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
								{r.name ?? r.host}
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
