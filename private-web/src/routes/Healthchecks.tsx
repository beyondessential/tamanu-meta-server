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
	ToggleButton,
	ToggleButtonGroup,
	Typography,
} from "@mui/material";
import { useMemo, useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { ApiError, useApi, useApiAction } from "../api";
import CheckResultChip from "../components/CheckResultChip";
import TimeAgo from "../components/TimeAgo";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { usePageTitle } from "../hooks/usePageTitle";
import {
	CEILINGS,
	healthcheckSettingsPath,
	type Ceiling,
	type CheckPolicyData,
	type IngestMode,
	type ReachabilityMode,
	type SourceData,
} from "../types";

/** A catalogued check unreported anywhere for this long is a
 * decommissioning candidate — mirrors the backend's 7-day window. */
const GONE_QUIET_MS = 7 * 24 * 60 * 60 * 1000;

function isGoneQuiet(row: CheckPolicyData): boolean {
	if (row.decommissioned_at || !row.last_seen) return false;
	return Date.now() - new Date(row.last_seen).getTime() > GONE_QUIET_MS;
}

export default function Healthchecks() {
	usePageTitle("Healthchecks");
	const isAdmin = useIsAdmin() === true;
	const list = useApi("healthchecks", "list");
	const [onlyPending, setOnlyPending] = useState(false);
	const [onlyGoneQuiet, setOnlyGoneQuiet] = useState(false);

	const rows: CheckPolicyData[] = list.status === "ok" ? list.data : [];
	const pendingCount = useMemo(
		() => rows.filter((r) => r.pending_review).length,
		[rows],
	);
	const goneQuietCount = useMemo(() => rows.filter(isGoneQuiet).length, [rows]);
	const visible = useMemo(
		() =>
			rows.filter(
				(r) =>
					(!onlyPending || r.pending_review) &&
					(!onlyGoneQuiet || isGoneQuiet(r)),
			),
		[rows, onlyPending, onlyGoneQuiet],
	);

	return (
		<Stack spacing={2}>
			<Box>
				<Typography variant="h6" component="h2" gutterBottom>
					Healthchecks
				</Typography>
				<Typography variant="body2" color="text.secondary">
					Catalog of healthchecks, one entry per reporting source and check
					name, with the ceiling each check's results are graded at. New
					checks land here at the default <strong>warning</strong> ceiling,
					marked pending review.
				</Typography>
			</Box>

			<SourcesSection isAdmin={isAdmin} />

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
				{goneQuietCount > 0 && (
					<Chip
						label={`${goneQuietCount} gone quiet`}
						color="warning"
						size="small"
						variant="outlined"
						title="Checks not reported anywhere in the fleet for 7+ days — candidates for decommissioning"
					/>
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
				<FormControlLabel
					control={
						<Switch
							size="small"
							checked={onlyGoneQuiet}
							onChange={(e) => setOnlyGoneQuiet(e.target.checked)}
						/>
					}
					label="Show only gone quiet"
				/>
			</Stack>

			{list.status === "loading" || list.status === "idle" ? (
				<LinearProgress />
			) : list.status === "error" ? (
				<Alert severity="error">{list.error.message}</Alert>
			) : visible.length === 0 ? (
				<Alert severity="info">
					{onlyGoneQuiet
						? "No checks have gone quiet."
						: onlyPending
							? "No checks pending review."
							: "No healthchecks reported yet."}
				</Alert>
			) : (
				<Paper variant="outlined">
					<TableContainer>
						<Table size="small">
							<TableHead>
								<TableRow>
									<TableCell>Source</TableCell>
									<TableCell>Check name</TableCell>
									<TableCell>Ceiling</TableCell>
									<TableCell>First seen</TableCell>
									<TableCell>Last seen</TableCell>
									<TableCell>Reviewed</TableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{visible.map((row) => (
									<HealthcheckRow
										key={`${row.source}:${row.check_name}`}
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

const REACHABILITY_MODES: ReachabilityMode[] = ["on", "quiet", "off"];
const INGEST_MODES: IngestMode[] = ["allow", "ignore", "deny"];

function SourcesSection({ isAdmin }: { isAdmin: boolean }) {
	const sources = useApi("healthchecks", "sources");
	const rows = sources.status === "ok" ? sources.data : [];
	// Nothing to show until at least one source has reported.
	if (sources.status === "ok" && rows.length === 0) return null;

	return (
		<Box>
			<Typography variant="subtitle2" component="h3" gutterBottom>
				Sources
			</Typography>
			<Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
				Reporters canopy expects to hear from. <strong>Reachability</strong>{" "}
				controls how a source going quiet affects its servers: <em>on</em>{" "}
				warns, <em>quiet</em> never warns but still marks a server unreachable
				when it's the only source left, <em>off</em> ignores it.{" "}
				<strong>Ingest</strong> controls the device API: <em>allow</em> accepts
				reports, <em>ignore</em> discards them, <em>deny</em> rejects the push.
				A source that isn't ingested can't count for reachability.
			</Typography>
			{sources.status === "loading" || sources.status === "idle" ? (
				<LinearProgress />
			) : sources.status === "error" ? (
				<Alert severity="error">{sources.error.message}</Alert>
			) : (
				<Paper variant="outlined">
					<TableContainer>
						<Table size="small">
							<TableHead>
								<TableRow>
									<TableCell>Source</TableCell>
									<TableCell>Last seen</TableCell>
									<TableCell>Reachability</TableCell>
									<TableCell>Ingest</TableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{rows.map((row) => (
									<SourceRow
										key={row.source}
										row={row}
										canEdit={isAdmin}
										onChanged={() => sources.reload()}
									/>
								))}
							</TableBody>
						</Table>
					</TableContainer>
				</Paper>
			)}
		</Box>
	);
}

function SourceRow({
	row,
	canEdit,
	onChanged,
}: {
	row: SourceData;
	canEdit: boolean;
	onChanged: () => void;
}) {
	const setReach = useApiAction("healthchecks", "set_source_reachability");
	const setIngest = useApiAction("healthchecks", "set_source_ingest");
	const [reach, setReachLocal] = useState<ReachabilityMode>(row.reachability);
	const [ingest, setIngestLocal] = useState<IngestMode>(row.ingest);

	const changeReach = async (mode: ReachabilityMode | null) => {
		if (!mode || mode === reach) return;
		const prev = reach;
		setReachLocal(mode);
		try {
			await setReach.call({ source: row.source, reachability: mode });
			onChanged();
		} catch {
			setReachLocal(prev);
		}
	};

	const changeIngest = async (mode: IngestMode | null) => {
		if (!mode || mode === ingest) return;
		const prev = ingest;
		setIngestLocal(mode);
		try {
			await setIngest.call({ source: row.source, ingest: mode });
			onChanged();
		} catch {
			setIngestLocal(prev);
		}
	};

	// A source that isn't ingested is excluded from reachability; show the
	// reachability control as a disabled "off".
	const ingested = ingest === "allow";
	const reachShown: ReachabilityMode = ingested ? reach : "off";

	return (
		<TableRow hover>
			<TableCell sx={{ fontFamily: "monospace" }}>{row.source}</TableCell>
			<TableCell>
				{row.last_seen ? (
					<TimeAgo timestamp={row.last_seen} />
				) : (
					<Typography variant="caption" color="text.secondary">
						never
					</Typography>
				)}
			</TableCell>
			<TableCell>
				{canEdit ? (
					<ToggleButtonGroup
						size="small"
						exclusive
						value={reachShown}
						onChange={(_, v) => changeReach(v as ReachabilityMode | null)}
						disabled={setReach.pending || !ingested}
					>
						{REACHABILITY_MODES.map((mode) => (
							<ToggleButton key={mode} value={mode}>
								{mode}
							</ToggleButton>
						))}
					</ToggleButtonGroup>
				) : (
					<Chip size="small" label={reachShown} />
				)}
				{setReach.error && (
					<Typography variant="caption" color="error" sx={{ display: "block" }}>
						{formatError(setReach.error)}
					</Typography>
				)}
			</TableCell>
			<TableCell>
				{canEdit ? (
					<ToggleButtonGroup
						size="small"
						exclusive
						value={ingest}
						onChange={(_, v) => changeIngest(v as IngestMode | null)}
						disabled={setIngest.pending}
					>
						{INGEST_MODES.map((mode) => (
							<ToggleButton key={mode} value={mode}>
								{mode}
							</ToggleButton>
						))}
					</ToggleButtonGroup>
				) : (
					<Chip size="small" label={ingest} />
				)}
				{setIngest.error && (
					<Typography variant="caption" color="error" sx={{ display: "block" }}>
						{formatError(setIngest.error)}
					</Typography>
				)}
			</TableCell>
		</TableRow>
	);
}

function HealthcheckRow({
	row,
	canEdit,
	onChanged,
}: {
	row: CheckPolicyData;
	canEdit: boolean;
	onChanged: () => void;
}) {
	const update = useApiAction("healthchecks", "update");
	const decommission = useApiAction("healthchecks", "decommission");
	const [localCeiling, setLocalCeiling] = useState<Ceiling>(row.ceiling as Ceiling);
	const [localEscalates, setLocalEscalates] = useState(row.escalates);

	const goneQuiet = isGoneQuiet(row);
	const decommissioned = row.decommissioned_at != null;

	const doDecommission = async () => {
		if (
			!window.confirm(
				`Decommission ${row.source}/${row.check_name}? Its states across all ` +
					`servers will be resolved and it will stop counting toward health ` +
					`and staleness. It returns pending review if reported again.`,
			)
		)
			return;
		try {
			await decommission.call({
				source: row.source,
				check_name: row.check_name,
			});
			onChanged();
		} catch {
			// Error is surfaced in the row below.
		}
	};

	const save = async () => {
		try {
			await update.call({
				source: row.source,
				check_name: row.check_name,
				ceiling: localCeiling,
				escalates: localEscalates,
				notes: row.notes,
			});
			onChanged();
		} catch {
			// Revert the editor selection on failure so the row's
			// rendered state matches the server's.
			setLocalCeiling(row.ceiling as Ceiling);
			setLocalEscalates(row.escalates);
		}
	};

	const hasRules = row.rule_count > 0;

	return (
		<TableRow hover>
			<TableCell sx={{ fontFamily: "monospace" }}>{row.source}</TableCell>
			<TableCell sx={{ fontFamily: "monospace" }}>
				<RouterLink to={healthcheckSettingsPath(row.source, row.check_name)}>
					{row.check_name}
				</RouterLink>
			</TableCell>
			<TableCell>
				{hasRules ? (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						<Typography variant="body2">
							<RouterLink to={healthcheckSettingsPath(row.source, row.check_name)}>
								Custom rules ({row.rule_count})
							</RouterLink>
						</Typography>
						<Typography variant="caption" color="text.secondary">
							· ceiling
						</Typography>
						<CheckResultChip result={row.ceiling as Ceiling} />
						{row.escalates && <EscalatesChip />}
					</Stack>
				) : (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						{canEdit ? (
							<>
								<Select
									size="small"
									value={localCeiling}
									onChange={(e) => setLocalCeiling(e.target.value as Ceiling)}
									disabled={update.pending}
									sx={{ minWidth: 120 }}
								>
									{CEILINGS.map((c) => (
										<MenuItem key={c} value={c}>
											<CheckResultChip result={c} />
										</MenuItem>
									))}
								</Select>
								<FormControlLabel
									control={
										<Switch
											size="small"
											checked={localEscalates}
											onChange={(e) => setLocalEscalates(e.target.checked)}
											disabled={update.pending}
										/>
									}
									label="escalates"
									slotProps={{ typography: { variant: "caption" } }}
								/>
							</>
						) : (
							<>
								<CheckResultChip result={row.ceiling as Ceiling} />
								{row.escalates && <EscalatesChip />}
							</>
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
							<RouterLink to={healthcheckSettingsPath(row.source, row.check_name)}>
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
				{decommissioned ? (
					<Chip
						label="decommissioned"
						size="small"
						variant="outlined"
						title={
							row.decommissioned_at
								? `Decommissioned ${new Date(row.decommissioned_at).toLocaleString()}`
								: undefined
						}
					/>
				) : (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						{row.last_seen ? (
							<TimeAgo timestamp={row.last_seen} />
						) : (
							<Typography variant="caption" color="text.secondary">
								never
							</Typography>
						)}
						{goneQuiet && (
							<Chip label="gone quiet" color="warning" size="small" />
						)}
						{canEdit && goneQuiet && (
							<Button
								size="small"
								color="warning"
								variant="outlined"
								onClick={doDecommission}
								disabled={decommission.pending}
							>
								Decommission
							</Button>
						)}
					</Stack>
				)}
				{decommission.error && (
					<Typography variant="caption" color="error" sx={{ display: "block" }}>
						{formatError(decommission.error)}
					</Typography>
				)}
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

function EscalatesChip() {
	return (
		<Chip
			label="escalates"
			color="error"
			size="small"
			variant="outlined"
			title="An effective failure notifies immediately, bypassing the incident grace period"
		/>
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
