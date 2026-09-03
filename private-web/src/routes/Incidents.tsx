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
	Stack,
	Switch,
	TextField,
	Typography,
} from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";
import { useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useApi } from "../api";
import IncidentCard from "../components/IncidentCard";
import IssueRow from "../components/IssueRow";
import { usePageTitle } from "../hooks/usePageTitle";
import { CHECK_RESULT_ORDER, type CheckResult } from "../types";

const DEFAULT_LIMIT = 200;

export default function Incidents() {
	usePageTitle("Incidents");

	// Page-level refresh signal: a single tick refetches incidents + issues
	// together after any mutation (resolve, snooze, manual submit).
	const [refreshTick, setRefreshTick] = useState(0);
	const bumpRefresh = () => setRefreshTick((t) => t + 1);

	// Filter state lives in the URL so links to a particular view round-trip.
	// Defaults aren't written into the URL (clean address bar for the
	// common case).
	const [params, setParams] = useSearchParams();
	const activeOnly = params.get("showAll") !== "1";
	const results = (params.get("result") ?? "")
		.split(",")
		.filter((s) => s.length > 0) as CheckResult[];
	const groupId = params.get("group") ?? "";

	const updateParam = (key: string, value: string | null) => {
		setParams(
			(prev) => {
				const next = new URLSearchParams(prev);
				if (value === null || value === "") next.delete(key);
				else next.set(key, value);
				return next;
			},
			{ replace: true },
		);
	};
	const setActiveOnly = (v: boolean) => updateParam("showAll", v ? null : "1");
	const setResults = (v: CheckResult[]) =>
		updateParam("result", v.length === 0 ? null : v.join(","));
	const setGroupId = (v: string) => updateParam("group", v === "" ? null : v);

	const incidents = useApi(
		"incidents",
		"list_active",
		{},
		[refreshTick],
	);
	const groups = useApi("fleet/groups", "list", {}, []);
	const issues = useApi(
		"issues",
		"list",
		{
			activeOnly,
			results: results.length === 0 ? null : results,
			serverGroupId: groupId === "" ? null : groupId,
			limit: DEFAULT_LIMIT,
		},
		[refreshTick, activeOnly, results.join(","), groupId],
	);

	return (
		<Stack spacing={3}>
			<Typography variant="h4" component="h1">
				Incidents
			</Typography>

			{incidents.status === "loading" || incidents.status === "idle" ? (
				<LinearProgress />
			) : incidents.status === "error" ? (
				<MuiAlert severity="error">{incidents.error.message}</MuiAlert>
			) : incidents.data.length === 0 ? (
				<MuiAlert severity="success">No open incidents.</MuiAlert>
			) : (
				<Box
					sx={{
						display: "grid",
						gridTemplateColumns: {
							xs: "1fr",
							sm: "repeat(2, 1fr)",
							md: "repeat(3, 1fr)",
							lg: "repeat(4, 1fr)",
						},
						gap: 2,
					}}
				>
					{incidents.data.map((inc) => (
						<IncidentCard key={inc.id} incident={inc} />
					))}
				</Box>
			)}

			<FilterBar
				activeOnly={activeOnly}
				setActiveOnly={setActiveOnly}
				results={results}
				setResults={setResults}
				groupId={groupId}
				setGroupId={setGroupId}
				groups={groups}
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
	results,
	setResults,
	groupId,
	setGroupId,
	groups,
	onRefresh,
}: {
	activeOnly: boolean;
	setActiveOnly: (v: boolean) => void;
	results: CheckResult[];
	setResults: (v: CheckResult[]) => void;
	groupId: string;
	setGroupId: (v: string) => void;
	groups: ReturnType<typeof useApi<"fleet/groups", "list">>;
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
					label="Results"
					value={results}
					onChange={(e) => {
						const v = e.target.value;
						setResults(
							typeof v === "string"
								? (v.split(",") as CheckResult[])
								: (v as CheckResult[]),
						);
					}}
					slotProps={{
						select: {
							multiple: true,
							renderValue: (selected) =>
								(selected as CheckResult[]).length === 0
									? "All"
									: (selected as CheckResult[]).join(", "),
						},
					}}
					sx={{ minWidth: 200 }}
				>
					{CHECK_RESULT_ORDER.map((r) => (
						<MenuItem key={r} value={r}>
							<Checkbox checked={results.includes(r)} size="small" />
							<ListItemText primary={r} />
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
					{groups.status === "ok" &&
						groups.data.map((g) => (
							<MenuItem key={g.id} value={g.id}>
								{g.name}
							</MenuItem>
						))}
				</TextField>
				<Box sx={{ ml: "auto" }}>
					<IconButton aria-label="Refresh" size="small" onClick={onRefresh}>
						<RefreshIcon fontSize="small" />
					</IconButton>
				</Box>
			</Stack>
		</Paper>
	);
}
