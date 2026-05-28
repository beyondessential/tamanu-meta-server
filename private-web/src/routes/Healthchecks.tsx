import {
	Alert,
	Box,
	Button,
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
import { Link as RouterLink } from "react-router-dom";
import { ApiError, useApi, useApiAction } from "../api";
import SeverityChip from "../components/SeverityChip";
import TimeAgo from "../components/TimeAgo";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { usePageTitle } from "../hooks/usePageTitle";
import { SEVERITIES, type HealthcheckSeverityData, type Severity } from "../types";

export default function Healthchecks() {
	usePageTitle("Healthchecks");
	const isAdmin = useIsAdmin() === true;
	const list = useApi("healthchecks", "list");
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
	const update = useApiAction("healthchecks", "update");
	const [localSeverity, setLocalSeverity] = useState<Severity>(row.severity);

	const save = async () => {
		try {
			await update.call({
				check_name: row.check_name,
				severity: localSeverity,
				notes: row.notes,
			});
			onChanged();
		} catch {
			// Revert the dropdown selection on failure so the row's
			// rendered state matches the server's.
			setLocalSeverity(row.severity);
		}
	};

	const hasRules = row.rule_count > 0;

	return (
		<TableRow hover>
			<TableCell sx={{ fontFamily: "monospace" }}>
				<RouterLink to={`/healthchecks/${row.check_name}`}>{row.check_name}</RouterLink>
			</TableCell>
			<TableCell>
				{hasRules ? (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						<Typography variant="body2">
							<RouterLink to={`/healthchecks/${row.check_name}`}>
								Custom rules ({row.rule_count})
							</RouterLink>
						</Typography>
						<Typography variant="caption" color="text.secondary">
							· base
						</Typography>
						<SeverityChip severity={row.severity} />
					</Stack>
				) : (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						{canEdit ? (
							<Select
								size="small"
								value={localSeverity}
								onChange={(e) => setLocalSeverity(e.target.value as Severity)}
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
						{canEdit && (
							<Button
								size="small"
								variant="outlined"
								onClick={save}
								disabled={update.pending}
							>
								Save
							</Button>
						)}
						<Typography variant="caption" color="text.secondary" sx={{ ml: 1 }}>
							·{" "}
							<RouterLink to={`/healthchecks/${row.check_name}`}>
								Add custom rules
							</RouterLink>
						</Typography>
					</Stack>
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
