import {
	Alert,
	Button,
	Divider,
	FormControlLabel,
	LinearProgress,
	Link as MuiLink,
	Paper,
	Stack,
	Step,
	StepLabel,
	Stepper,
	Switch,
	TextField,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { Link as RouterLink, useNavigate, useParams } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import {
	RETENTION_FLOORS,
	type BackupConfigView,
	type BackupRepoMode,
	type RetentionPolicy,
} from "../types";

const DEFAULT_RETENTION: RetentionPolicy = {
	keep_latest: 1,
	keep_daily: 7,
	keep_weekly: 4,
	keep_monthly: 6,
	keep_annual: 0,
};

/// Most buckets live here, so default the region to it.
const DEFAULT_REGION = "ap-southeast-2";
const WELL_KNOWN_TYPE = "tamanu-postgres";

/// Probe response shape (wire type is generated; this is the UI-facing subset).
type ProbeResult = {
	state: "empty" | "kopia_repo" | "other_content" | "inaccessible";
	error: string | null;
	object_sample: string[];
	already_configured: string | null;
};

/// Onboarding (create) / edit form for a group's backup config. Create is a
/// probe-driven wizard: enter the bucket + roles, Canopy inspects it, and the
/// repo mode is derived from what's there (empty → from-birth; existing kopia
/// repo → import by passphrase; other content / inaccessible → blocked). Edit is
/// a flat form (structural fields are create-only).
export default function BackupConfig() {
	const { id = "" } = useParams<{ id: string }>();
	const existing = useApi("backups", "get", { server_group_id: id }, [id]);
	usePageTitle("Backup config");

	if (existing.status === "loading" || existing.status === "idle") {
		return <LinearProgress />;
	}
	if (existing.status === "error") {
		return <Alert severity="error">{existing.error.message}</Alert>;
	}
	return <ConfigForm groupId={id} existing={existing.data} />;
}

function ConfigForm({
	groupId,
	existing,
}: {
	groupId: string;
	existing: BackupConfigView | null;
}) {
	const navigate = useNavigate();
	const isCreate = existing == null;
	const create = useApiAction("backups", "create");
	const update = useApiAction("backups", "update");
	const setSchedule = useApiAction("backups", "set_schedule");
	const createRepo = useApiAction("backups", "create_repo");
	const probeAction = useApiAction("backups", "probe");

	const wellKnown = existing?.schedules.find((s) => s.type === WELL_KNOWN_TYPE);

	const [bucket, setBucket] = useState(existing?.bucket ?? "");
	const [prefix, setPrefix] = useState(existing?.prefix ?? "");
	const [roleArn, setRoleArn] = useState(existing?.target_role_arn ?? "");
	const [maintenanceRoleArn, setMaintenanceRoleArn] = useState(
		existing?.maintenance_role_arn ?? "",
	);
	const [region, setRegion] = useState(existing?.region ?? DEFAULT_REGION);
	const [passphrase, setPassphrase] = useState("");
	const [scheduled, setScheduled] = useState(
		wellKnown ? wellKnown.expected_interval != null : true,
	);
	const [intervalMinutes, setIntervalMinutes] = useState<string>(
		wellKnown?.expected_interval != null
			? Math.max(1, Math.round(wellKnown.expected_interval / 60)).toString()
			: "60",
	);
	const initialRetention = wellKnown?.retention ?? DEFAULT_RETENTION;
	const [retention, setRetention] = useState<RetentionPolicy>(initialRetention);

	// Wizard state (create only).
	const [step, setStep] = useState(0);
	const [probe, setProbe] = useState<ProbeResult | null>(null);

	const floorErrors = retentionFloorErrors(retention);
	const hasFloorError = floorErrors.length > 0;

	// Probe-derived repo mode: an existing kopia repo is imported by passphrase;
	// an empty bucket is created from-birth. (Canopy never lets the operator pick
	// the passphrase for a repo it creates.)
	const mode: BackupRepoMode =
		probe?.state === "kopia_repo" ? "passphrase" : "from_birth";

	const pending =
		create.pending ||
		update.pending ||
		setSchedule.pending ||
		createRepo.pending;
	const error =
		create.error || update.error || setSchedule.error || createRepo.error;

	const runProbe = async () => {
		try {
			const result = (await probeAction.call({
				bucket,
				prefix,
				region: region.trim() === "" ? null : region,
				maintenance_role_arn: maintenanceRoleArn,
			})) as ProbeResult;
			setProbe(result);
			setStep(1);
		} catch {
			/* surfaced via probeAction.error */
		}
	};

	const persist = async () => {
		try {
			if (isCreate) {
				await create.call({
					server_group_id: groupId,
					bucket,
					prefix,
					target_role_arn: roleArn,
					maintenance_role_arn: maintenanceRoleArn,
					region: region.trim() === "" ? null : region,
					mode,
					passphrase: mode === "passphrase" ? passphrase : null,
				});
			} else {
				await update.call({
					server_group_id: groupId,
					region: region.trim() === "" ? null : region,
				});
			}
			await setSchedule.call({
				server_group_id: groupId,
				type: WELL_KNOWN_TYPE,
				expected_interval: scheduled
					? Math.max(60, Math.round(Number(intervalMinutes) * 60))
					: null,
				retention,
			});
			if (isCreate) {
				await createRepo.call({ server_group_id: groupId });
			}
			navigate(`/groups/${groupId}/backups`);
		} catch {
			/* surfaced via the action errors */
		}
	};

	const scheduleAndRetention = (
		<>
			<Typography variant="subtitle1">Schedule</Typography>
			<FormControlLabel
				control={
					<Switch
						checked={scheduled}
						onChange={(e) => setScheduled(e.target.checked)}
						disabled={pending}
					/>
				}
				label={scheduled ? "Scheduled" : "Manual only (no schedule)"}
			/>
			{scheduled && (
				<TextField
					label="Back up every (minutes)"
					type="number"
					value={intervalMinutes}
					onChange={(e) => setIntervalMinutes(e.target.value)}
					disabled={pending}
					slotProps={{ htmlInput: { min: 1, step: 1 } }}
					sx={{ width: 220 }}
				/>
			)}

			<Divider />

			<Typography variant="subtitle1">Retention</Typography>
			<Typography variant="caption" color="text.secondary">
				kopia keep-* policy. Org minimums: keep_daily ≥{" "}
				{RETENTION_FLOORS.keep_daily}, keep_weekly ≥{" "}
				{RETENTION_FLOORS.keep_weekly}, keep_monthly ≥{" "}
				{RETENTION_FLOORS.keep_monthly}.
			</Typography>
			<Stack direction={{ xs: "column", md: "row" }} spacing={2}>
				<RetentionField
					label="Latest"
					value={retention.keep_latest}
					onChange={(v) => setRetention({ ...retention, keep_latest: v })}
					disabled={pending}
				/>
				<RetentionField
					label="Daily"
					value={retention.keep_daily}
					onChange={(v) => setRetention({ ...retention, keep_daily: v })}
					disabled={pending}
					floor={RETENTION_FLOORS.keep_daily}
				/>
				<RetentionField
					label="Weekly"
					value={retention.keep_weekly}
					onChange={(v) => setRetention({ ...retention, keep_weekly: v })}
					disabled={pending}
					floor={RETENTION_FLOORS.keep_weekly}
				/>
				<RetentionField
					label="Monthly"
					value={retention.keep_monthly}
					onChange={(v) => setRetention({ ...retention, keep_monthly: v })}
					disabled={pending}
					floor={RETENTION_FLOORS.keep_monthly}
				/>
				<RetentionField
					label="Annual"
					value={retention.keep_annual}
					onChange={(v) => setRetention({ ...retention, keep_annual: v })}
					disabled={pending}
				/>
			</Stack>
			{hasFloorError && (
				<Alert severity="warning">{floorErrors.join("; ")}</Alert>
			)}
		</>
	);

	// ── Edit mode: flat form (structural fields are create-only) ───────────────
	if (!isCreate) {
		return (
			<Paper variant="outlined" sx={{ p: 3 }}>
				<Stack spacing={2}>
					<Typography variant="h5" component="h1">
						Edit backup config
					</Typography>
					<TextField label="Bucket" value={bucket} disabled />
					<TextField label="Target role ARN" value={roleArn} disabled />
					<TextField
						label="Maintenance role ARN"
						value={maintenanceRoleArn}
						disabled
					/>
					<TextField
						label="Region"
						value={region}
						onChange={(e) => setRegion(e.target.value)}
						disabled={pending}
						helperText="Changing region typically implies a different bucket — change with care."
					/>
					<Divider />
					{scheduleAndRetention}
					{error && <Alert severity="error">{error.message}</Alert>}
					<Stack direction="row" spacing={1}>
						<Button
							variant="contained"
							onClick={persist}
							disabled={pending || hasFloorError}
						>
							{pending ? "Saving…" : "Save"}
						</Button>
						<Button
							variant="outlined"
							color="error"
							onClick={() => navigate(`/groups/${groupId}/backups`)}
							disabled={pending}
						>
							Cancel
						</Button>
					</Stack>
				</Stack>
			</Paper>
		);
	}

	// ── Create mode: probe-driven wizard ───────────────────────────────────────
	const canCheck =
		bucket.trim() !== "" &&
		roleArn.trim() !== "" &&
		maintenanceRoleArn.trim() !== "";
	const canContinueReview =
		probe != null &&
		probe.already_configured == null &&
		(probe.state === "empty" ||
			(probe.state === "kopia_repo" && passphrase.trim() !== ""));

	return (
		<Paper variant="outlined" sx={{ p: 3 }}>
			<Stack spacing={3}>
				<Typography variant="h5" component="h1">
					Set up backups
				</Typography>
				<Stepper activeStep={step}>
					<Step>
						<StepLabel>Target bucket</StepLabel>
					</Step>
					<Step>
						<StepLabel>Review</StepLabel>
					</Step>
					<Step>
						<StepLabel>Schedule &amp; retention</StepLabel>
					</Step>
				</Stepper>

				{step === 0 && (
					<Stack spacing={2}>
						<TextField
							label="Bucket"
							value={bucket}
							onChange={(e) => setBucket(e.target.value)}
							disabled={probeAction.pending}
							required
							helperText="S3 bucket holding this group's kopia repository."
						/>
						<TextField
							label="Prefix"
							value={prefix}
							onChange={(e) => setPrefix(e.target.value)}
							disabled={probeAction.pending}
							helperText="Optional key prefix within the bucket."
						/>
						<TextField
							label="Region"
							value={region}
							onChange={(e) => setRegion(e.target.value)}
							disabled={probeAction.pending}
							helperText="Defaults to ap-southeast-2."
						/>
						<TextField
							label="Target role ARN"
							value={roleArn}
							onChange={(e) => setRoleArn(e.target.value)}
							disabled={probeAction.pending}
							required
							helperText="IAM role Canopy assumes to mint device credentials (no delete)."
						/>
						<TextField
							label="Maintenance role ARN"
							value={maintenanceRoleArn}
							onChange={(e) => setMaintenanceRoleArn(e.target.value)}
							disabled={probeAction.pending}
							required
							helperText="IAM role for maintenance + this check (s3:* + delete + CloudWatch)."
						/>
						{probeAction.error && (
							<Alert severity="error">{probeAction.error.message}</Alert>
						)}
						<Stack direction="row" spacing={1}>
							<Button
								variant="contained"
								onClick={runProbe}
								disabled={!canCheck || probeAction.pending}
							>
								{probeAction.pending ? "Checking…" : "Check bucket"}
							</Button>
							<Button
								variant="outlined"
								color="error"
								onClick={() => navigate(`/groups/${groupId}/backups`)}
								disabled={probeAction.pending}
							>
								Cancel
							</Button>
						</Stack>
					</Stack>
				)}

				{step === 1 && probe && (
					<Stack spacing={2}>
						<ProbeReview
							probe={probe}
							groupId={groupId}
							passphrase={passphrase}
							setPassphrase={setPassphrase}
						/>
						<Stack direction="row" spacing={1}>
							<Button variant="outlined" onClick={() => setStep(0)}>
								Back
							</Button>
							{(probe.state === "other_content" ||
								probe.state === "inaccessible") && (
								<Button variant="contained" onClick={runProbe}>
									Re-check
								</Button>
							)}
							<Button
								variant="contained"
								onClick={() => setStep(2)}
								disabled={!canContinueReview}
							>
								Continue
							</Button>
						</Stack>
					</Stack>
				)}

				{step === 2 && (
					<Stack spacing={2}>
						{scheduleAndRetention}
						{error && <Alert severity="error">{error.message}</Alert>}
						<Stack direction="row" spacing={1}>
							<Button
								variant="outlined"
								onClick={() => setStep(1)}
								disabled={pending}
							>
								Back
							</Button>
							<Button
								variant="contained"
								onClick={persist}
								disabled={pending || hasFloorError}
							>
								{pending ? "Saving…" : "Create & provision"}
							</Button>
						</Stack>
					</Stack>
				)}
			</Stack>
		</Paper>
	);
}

/// Step-2 review: explain what the probe found and gate the next action.
function ProbeReview({
	probe,
	groupId,
	passphrase,
	setPassphrase,
}: {
	probe: ProbeResult;
	groupId: string;
	passphrase: string;
	setPassphrase: (v: string) => void;
}) {
	if (
		probe.already_configured != null &&
		probe.already_configured !== groupId
	) {
		return (
			<Alert severity="error">
				This bucket + prefix is already configured for another group.{" "}
				<MuiLink
					component={RouterLink}
					to={`/groups/${probe.already_configured}/backups`}
				>
					View that config
				</MuiLink>
				. Use a different prefix or bucket.
			</Alert>
		);
	}
	switch (probe.state) {
		case "inaccessible":
			return (
				<Alert severity="error">
					Couldn't access the bucket: {probe.error ?? "unknown error"}. Check
					the role ARN, bucket, and region, then re-check.
				</Alert>
			);
		case "other_content":
			return (
				<Alert severity="warning">
					The bucket/prefix already holds other (non-kopia) content
					{probe.object_sample.length > 0 && (
						<> — e.g. {probe.object_sample.join(", ")}</>
					)}
					. Canopy won't write over it: clear the prefix, or use a different
					prefix/bucket, then re-check.
				</Alert>
			);
		case "empty":
			return (
				<Alert severity="info">
					Empty bucket. Canopy will create a new repository and generate its
					passphrase (from-birth), then rotate it regularly.
				</Alert>
			);
		case "kopia_repo":
			return (
				<Stack spacing={2}>
					<Alert severity="info">
						An existing kopia repository is here. Supply its passphrase to
						import it; Canopy verifies it at init and then rotates it to a
						Canopy-owned passphrase (a hard break for any existing consumers).
					</Alert>
					<TextField
						label="Existing repository passphrase"
						type="password"
						value={passphrase}
						onChange={(e) => setPassphrase(e.target.value)}
						required
					/>
				</Stack>
			);
	}
}

function RetentionField({
	label,
	value,
	onChange,
	disabled,
	floor,
}: {
	label: string;
	value: number;
	onChange: (v: number) => void;
	disabled: boolean;
	floor?: number;
}) {
	const below = floor != null && value < floor;
	return (
		<TextField
			label={label}
			type="number"
			value={value}
			onChange={(e) => onChange(Number(e.target.value))}
			disabled={disabled}
			error={below}
			helperText={floor != null ? `≥ ${floor}` : undefined}
			slotProps={{ htmlInput: { min: floor ?? 0, step: 1 } }}
			sx={{ width: 110 }}
		/>
	);
}

function retentionFloorErrors(r: RetentionPolicy): string[] {
	const errs: string[] = [];
	if (r.keep_daily < RETENTION_FLOORS.keep_daily)
		errs.push(`keep_daily must be ≥ ${RETENTION_FLOORS.keep_daily}`);
	if (r.keep_weekly < RETENTION_FLOORS.keep_weekly)
		errs.push(`keep_weekly must be ≥ ${RETENTION_FLOORS.keep_weekly}`);
	if (r.keep_monthly < RETENTION_FLOORS.keep_monthly)
		errs.push(`keep_monthly must be ≥ ${RETENTION_FLOORS.keep_monthly}`);
	return errs;
}
