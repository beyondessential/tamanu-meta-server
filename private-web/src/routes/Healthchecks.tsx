import {
	Alert,
	Box,
	Chip,
	FormControlLabel,
	LinearProgress,
	MenuItem,
	Paper,
	Select,
	Stack,
	Switch,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	Typography,
} from "@mui/material";
import { useMemo, useState } from "react";
import { ApiError, useApi, useApiAction } from "../api";
import SeverityChip from "../components/SeverityChip";
import TimeAgo from "../components/TimeAgo";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { usePageTitle } from "../hooks/usePageTitle";
import { SEVERITIES, type HealthcheckSeverityData, type Severity } from "../types";

export default function Healthchecks() {
	usePageTitle("Healthchecks");
	const isAdmin = useIsAdmin() === true;
	const list = useApi("healthchecks", "list_severities");
	const [onlyPending, setOnlyPending] = useState(false);

	const rows: HealthcheckSeverityData[] = list.status === "ok" ? list.data : [];
	const pendingCount = useMemo(
		() => rows.filter((r) => r.pending_review).length,
		[rows],
	);
	const visible = useMemo(
		() => (onlyPending ? rows.filter((r) => r.pending_review) : rows),
		[rows, onlyPending],
	);

	return (
		<Stack spacing={2}>
			<Box>
				<Typography variant="h6" component="h2" gutterBottom>
					Healthchecks
				</Typography>
				<Typography variant="body2" color="text.secondary">
					Catalog of healthcheck names reported by servers and the severity
					each check's failures are filed at. New checks land here at the
					default <strong>warning</strong> severity, marked pending review.
				</Typography>
			</Box>

			<Stack direction="row" spacing={2} sx={{ alignItems: "center" }}>
				{pendingCount > 0 ? (
					<Chip
						label={`${pendingCount} pending review`}
						color="warning"
						size="small"
					/>
				) : (
					<Chip label="all reviewed" color="success" size="small" variant="outlined" />
				)}
				<FormControlLabel
					control={
						<Switch
							size="small"
							checked={onlyPending}
							onChange={(e) => setOnlyPending(e.target.checked)}
						/>
					}
					label="Show only pending review"
				/>
			</Stack>

			{list.status === "loading" || list.status === "idle" ? (
				<LinearProgress />
			) : list.status === "error" ? (
				<Alert severity="error">{list.error.message}</Alert>
			) : visible.length === 0 ? (
				<Alert severity="info">
					{onlyPending
						? "No checks pending review."
						: "No healthchecks reported yet."}
				</Alert>
			) : (
				<Paper variant="outlined">
					<TableContainer>
						<Table size="small">
							<TableHead>
								<TableRow>
									<TableCell>Check name</TableCell>
									<TableCell>Severity</TableCell>
									<TableCell>First seen</TableCell>
									<TableCell>Reviewed</TableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{visible.map((row) => (
									<HealthcheckRow
										key={row.check_name}
										row={row}
										canEdit={isAdmin}
										onChanged={() => list.reload()}
									/>
								))}
							</TableBody>
						</Table>
					</TableContainer>
				</Paper>
			)}
		</Stack>
	);
}

function HealthcheckRow({
	row,
	canEdit,
	onChanged,
}: {
	row: HealthcheckSeverityData;
	canEdit: boolean;
	onChanged: () => void;
}) {
	const update = useApiAction("healthchecks", "update_severity");
	const [localSeverity, setLocalSeverity] = useState<Severity>(row.severity);

	const onChange = async (next: Severity) => {
		setLocalSeverity(next);
		try {
			await update.call({
				check_name: row.check_name,
				severity: next,
				notes: row.notes,
			});
			onChanged();
		} catch {
			// Revert local optimistic value on failure.
			setLocalSeverity(row.severity);
		}
	};

	return (
		<TableRow hover>
			<TableCell sx={{ fontFamily: "monospace" }}>{row.check_name}</TableCell>
			<TableCell>
				{canEdit ? (
					<Select
						size="small"
						value={localSeverity}
						onChange={(e) => onChange(e.target.value as Severity)}
						disabled={update.pending}
						sx={{ minWidth: 120 }}
					>
						{SEVERITIES.map((s) => (
							<MenuItem key={s} value={s}>
								<SeverityChip severity={s} />
							</MenuItem>
						))}
					</Select>
				) : (
					<SeverityChip severity={row.severity} />
				)}
				{update.error && (
					<Typography variant="caption" color="error" sx={{ display: "block" }}>
						{formatError(update.error)}
					</Typography>
				)}
			</TableCell>
			<TableCell>
				<TimeAgo timestamp={row.first_seen} />
			</TableCell>
			<TableCell>
				{row.pending_review ? (
					<Chip label="pending review" color="warning" size="small" />
				) : (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						{row.reviewed_at && <TimeAgo timestamp={row.reviewed_at} />}
						{row.reviewed_by && (
							<Typography variant="caption" color="text.secondary">
								by {row.reviewed_by}
							</Typography>
						)}
					</Stack>
				)}
			</TableCell>
		</TableRow>
	);
}

function formatError(err: unknown): string {
	if (err instanceof ApiError) {
		const detail = err.detail as { title?: string } | null;
		if (detail?.title) return detail.title;
		return err.message;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}
