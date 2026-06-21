import {
	Alert,
	Button,
	Divider,
	FormControlLabel,
	LinearProgress,
	MenuItem,
	Paper,
	Stack,
	Switch,
	TextField,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import {
	BACKUP_MODE_LABEL,
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

const WELL_KNOWN_TYPE = "tamanu-postgres";

/// Onboarding (create) / edit form for a group's backup config. Structural
/// fields (bucket, role, mode) are create-only; in edit mode they're shown
/// disabled. Interval + retention are persisted per-(group,type) via
/// `set_schedule` after the config row exists.
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

	const wellKnown = existing?.schedules.find((s) => s.type === WELL_KNOWN_TYPE);

	const [bucket, setBucket] = useState(existing?.bucket ?? "");
	const [prefix, setPrefix] = useState(existing?.prefix ?? "");
	const [roleArn, setRoleArn] = useState(existing?.target_role_arn ?? "");
	const [maintenanceRoleArn, setMaintenanceRoleArn] = useState(
		existing?.maintenance_role_arn ?? "",
	);
	const [region, setRegion] = useState(existing?.region ?? "");
	const [mode, setMode] = useState<BackupRepoMode>(
		(existing?.mode as BackupRepoMode) ?? "from_birth",
	);
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

	const floorErrors = retentionFloorErrors(retention);
	const hasFloorError = floorErrors.length > 0;

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		if (hasFloorError) return;
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
				// Kick repo init (provisioning → ready). Canopy owns + rotates the
				// passphrase, so there's no escrow step either way.
				await createRepo.call({ server_group_id: groupId });
			}
			navigate(`/groups/${groupId}/backups`);
		} catch {
			/* surfaced via the action errors */
		}
	};

	const pending =
		create.pending ||
		update.pending ||
		setSchedule.pending ||
		createRepo.pending;
	const error =
		create.error || update.error || setSchedule.error || createRepo.error;

	return (
		<Paper
			variant="outlined"
			sx={{ p: 3 }}
			component="form"
			onSubmit={onSubmit}
		>
			<Stack spacing={2}>
				<Typography variant="h5" component="h1">
					{isCreate ? "Set up backups" : "Edit backup config"}
				</Typography>

				<TextField
					label="Bucket"
					value={bucket}
					onChange={(e) => setBucket(e.target.value)}
					disabled={pending || !isCreate}
					required
					helperText={
						isCreate
							? "S3 bucket holding this group's kopia repository."
							: "Changing the bucket is a repo migration — out of scope here."
					}
				/>
				<TextField
					label="Prefix"
					value={prefix}
					onChange={(e) => setPrefix(e.target.value)}
					disabled={pending || !isCreate}
					helperText="Optional key prefix within the bucket."
				/>
				<TextField
					label="Target role ARN"
					value={roleArn}
					onChange={(e) => setRoleArn(e.target.value)}
					disabled={pending || !isCreate}
					required
					helperText={
						isCreate
							? "IAM role Canopy assumes to mint device credentials."
							: "Changing the role is a repo migration — out of scope here."
					}
				/>
				<TextField
					label="Maintenance role ARN"
					value={maintenanceRoleArn}
					onChange={(e) => setMaintenanceRoleArn(e.target.value)}
					disabled={pending || !isCreate}
					required
					helperText={
						isCreate
							? "IAM role the backups pod assumes for maintenance (s3:* + delete + CloudWatch)."
							: "Changing the role is a repo migration — out of scope here."
					}
				/>
				<TextField
					label="Region"
					value={region}
					onChange={(e) => setRegion(e.target.value)}
					disabled={pending}
					helperText="Optional. Changing region typically implies a different bucket — change with care."
				/>

				<TextField
					select
					label="Repository mode"
					value={mode}
					onChange={(e) => setMode(e.target.value as BackupRepoMode)}
					disabled={pending || !isCreate}
					helperText={
						isCreate
							? "From birth: Canopy generates the passphrase for a new repo. Existing repository: connect by supplying its passphrase."
							: "Mode is fixed after creation."
					}
				>
					<MenuItem value="from_birth">
						{BACKUP_MODE_LABEL.from_birth}
					</MenuItem>
					<MenuItem value="passphrase">
						{BACKUP_MODE_LABEL.passphrase}
					</MenuItem>
				</TextField>

				{isCreate && mode === "passphrase" && (
					<TextField
						label="Existing repository passphrase"
						type="password"
						value={passphrase}
						onChange={(e) => setPassphrase(e.target.value)}
						disabled={pending}
						required
						helperText="Connect to an existing kopia repository by supplying its passphrase. New repositories use “From birth” (Canopy generates the passphrase)."
					/>
				)}

				<Divider />

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

				{error && <Alert severity="error">{error.message}</Alert>}

				<Stack direction="row" spacing={1}>
					<Button
						type="submit"
						variant="contained"
						disabled={pending || hasFloorError}
					>
						{pending
							? "Saving…"
							: isCreate
								? "Create & provision"
								: "Save"}
					</Button>
					<Button
						type="button"
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
