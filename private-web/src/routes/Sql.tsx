import {
	Alert,
	Box,
	Button,
	Chip,
	IconButton,
	LinearProgress,
	Pagination,
	Paper,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	Typography,
} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import { useState } from "react";
import { useApi, useApiAction } from "../api";
import SqlEditor from "../components/SqlEditor";
import { usePageTitle } from "../hooks/usePageTitle";
import type { Page, SqlHistoryEntry, SqlResult } from "../types";

const HISTORY_PAGE_SIZE = 10;

export default function Sql() {
	usePageTitle("SQL Playground");
	const [query, setQuery] = useState("");
	const [result, setResult] = useState<SqlResult | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [historyOpen, setHistoryOpen] = useState(false);
	const [historyPage, setHistoryPage] = useState(0);

	const executeAction = useApiAction<SqlResult>("sql", "execute_query");
	const lastQueryAction = useApiAction<string | null>(
		"sql",
		"get_last_user_query",
	);

	const history = useApi<Page<SqlHistoryEntry>>(
		"sql",
		"get_query_history",
		{ offset: historyPage * HISTORY_PAGE_SIZE, limit: HISTORY_PAGE_SIZE },
		[historyPage, result],
	);

	const run = async () => {
		const text = query.trim();
		if (!text) return;
		setError(null);
		setResult(null);
		try {
			const r = await executeAction.call({ query: { query: text } });
			setResult(r);
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
		}
	};

	const loadLastQuery = async () => {
		try {
			const last = await lastQueryAction.call({});
			if (last) setQuery(last);
		} catch {
			/* ignore */
		}
	};

	const recall = (q: string) => setQuery(q);

	const totalHistory = history.status === "ok" ? history.data.total : 0;
	const historyPageCount = Math.max(
		1,
		Math.ceil(totalHistory / HISTORY_PAGE_SIZE),
	);

	return (
		<Stack spacing={3}>
			<Typography variant="h4" component="h1">
				SQL Playground
			</Typography>

			<Box
				onKeyUp={(e) => {
					if (e.key === "Enter" && e.ctrlKey) void run();
				}}
				sx={{
					border: 1,
					borderColor: "divider",
					borderRadius: 1,
					overflow: "hidden",
				}}
			>
				<SqlEditor
					value={query}
					onChange={setQuery}
					readOnly={executeAction.pending}
					placeholder="SELECT * FROM statuses ORDER BY created_at DESC LIMIT 10;"
					minHeight="14em"
				/>
			</Box>

			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<Button
					variant="contained"
					onClick={run}
					disabled={!query.trim() || executeAction.pending}
				>
					{executeAction.pending ? "Running…" : "Run"}
				</Button>
				<Box sx={{ flex: 1 }} />
				<Button
					variant="outlined"
					onClick={loadLastQuery}
					disabled={executeAction.pending || lastQueryAction.pending}
				>
					↶ Last query
				</Button>
				<Button
					variant="outlined"
					onClick={() => setHistoryOpen((v) => !v)}
					disabled={executeAction.pending}
				>
					History {historyOpen ? "▼" : "▶"}
				</Button>
			</Stack>

			{historyOpen && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					{history.status === "loading" || history.status === "idle" ? (
						<LinearProgress />
					) : history.status === "error" ? (
						<Alert severity="error">{history.error.message}</Alert>
					) : history.data.items.length === 0 ? (
						<Alert severity="info">No history available.</Alert>
					) : (
						<TableContainer>
							<Table size="small">
								<TableHead>
									<TableRow>
										<TableCell>When</TableCell>
										<TableCell>Who</TableCell>
										<TableCell>What</TableCell>
										<TableCell />
									</TableRow>
								</TableHead>
								<TableBody>
									{history.data.items.map((entry) => (
										<TableRow key={entry.id}>
											<TableCell>
												{new Date(entry.created_at).toLocaleString()}
											</TableCell>
											<TableCell>
												<Chip
													size="small"
													color="info"
													label={entry.tailscale_user}
												/>
											</TableCell>
											<TableCell sx={{ fontFamily: "monospace" }}>
												{entry.query}
											</TableCell>
											<TableCell>
												<Button
													size="small"
													variant="outlined"
													onClick={() => recall(entry.query)}
												>
													Recall
												</Button>
											</TableCell>
										</TableRow>
									))}
								</TableBody>
							</Table>
						</TableContainer>
					)}
					{historyPageCount > 1 && (
						<Box
							sx={{
								mt: 1,
								display: "flex",
								justifyContent: "center",
							}}
						>
							<Pagination
								count={historyPageCount}
								page={historyPage + 1}
								onChange={(_, p) => setHistoryPage(p - 1)}
							/>
						</Box>
					)}
				</Paper>
			)}

			{error && (
				<Alert
					severity="error"
					action={
						<IconButton
							aria-label="dismiss error"
							size="small"
							onClick={() => setError(null)}
						>
							<CloseIcon fontSize="small" />
						</IconButton>
					}
				>
					<strong>Error executing query:</strong>
					<br />
					{error}
				</Alert>
			)}

			{result && <ResultDisplay result={result} />}
		</Stack>
	);
}

function ResultDisplay({ result }: { result: SqlResult }) {
	return (
		<Stack spacing={2}>
			<Alert severity="info">
				<strong>Query executed successfully!</strong> Returned{" "}
				<strong>{result.row_count}</strong> rows in{" "}
				<strong>{result.execution_time_ms}</strong> ms.
			</Alert>
			<TableContainer component={Paper} variant="outlined">
				<Table size="small">
					<TableHead>
						<TableRow>
							{result.columns.map((c) => (
								<TableCell key={c}>{c}</TableCell>
							))}
						</TableRow>
					</TableHead>
					<TableBody>
						{result.rows.map((row, i) => (
							<TableRow key={i}>
								{row.map((cell, j) => (
									<TableCell key={j}>
										<JsonCell value={cell} />
									</TableCell>
								))}
							</TableRow>
						))}
					</TableBody>
				</Table>
			</TableContainer>
			{result.row_count === 0 && (
				<Alert severity="warning">No rows returned by the query.</Alert>
			)}
		</Stack>
	);
}

function JsonCell({ value }: { value: unknown }) {
	if (value === null || value === undefined)
		return (
			<Typography component="span" color="text.secondary">
				NULL
			</Typography>
		);
	if (typeof value === "boolean")
		return (
			<Typography component="span" color="info.main">
				{value.toString()}
			</Typography>
		);
	if (typeof value === "number")
		return (
			<Typography component="span" color="primary.main">
				{value.toString()}
			</Typography>
		);
	if (typeof value === "string")
		return (
			<Typography component="span" color="success.main">
				{value}
			</Typography>
		);
	if (Array.isArray(value))
		return (
			<Typography component="span" color="warning.main">
				[
				{value.map((v, i) => (
					<span key={i}>
						<JsonCell value={v} />
						{i < value.length - 1 ? ", " : ""}
					</span>
				))}
				]
			</Typography>
		);
	return (
		<Box
			component="pre"
			sx={{
				m: 0,
				fontFamily: "monospace",
				fontSize: "0.85em",
			}}
		>
			{JSON.stringify(value, null, 2)}
		</Box>
	);
}
