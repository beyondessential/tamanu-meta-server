import {
	Alert,
	Box,
	Button,
	FormControlLabel,
	LinearProgress,
	Paper,
	Stack,
	Switch,
	TextField,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { useApi, useApiAction } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

type Retention = {
	keep_latest: number;
	keep_daily: number;
	keep_weekly: number;
	keep_monthly: number;
	keep_annual: number;
};

type TypeDefault = {
	type: string;
	default_interval: number | null;
	default_retention: Retention | null;
	auto_enable: boolean;
};

const FLOOR_RETENTION: Retention = {
	keep_latest: 1,
	keep_daily: 7,
	keep_weekly: 4,
	keep_monthly: 6,
	keep_annual: 0,
};

const RETENTION_FIELDS: Array<{ key: keyof Retention; label: string; floor?: number }> = [
	{ key: "keep_latest", label: "Latest" },
	{ key: "keep_daily", label: "Daily", floor: 7 },
	{ key: "keep_weekly", label: "Weekly", floor: 4 },
	{ key: "keep_monthly", label: "Monthly", floor: 6 },
	{ key: "keep_annual", label: "Annual" },
];

/// Canopy-wide per-type backup defaults (`backup_type_defaults`): the schedule +
/// retention each group inherits for a type unless it sets a per-group override.
export default function BackupDefaults() {
	usePageTitle("Backup defaults");
	const defaults = useApi("backups", "type_defaults");

	if (defaults.status === "loading" || defaults.status === "idle") {
		return <LinearProgress />;
	}
	if (defaults.status === "error") {
		return <Alert severity="error">{defaults.error.message}</Alert>;
	}

	return (
		<Stack spacing={3}>
			<Typography variant="body2" color="text.secondary">
				The canopy-wide default schedule and retention for each backup type.
				Groups inherit these; per-group overrides are set on a group's backup
				page.
			</Typography>
			{defaults.data.length === 0 ? (
				<Alert severity="info">No backup type defaults configured.</Alert>
			) : (
				defaults.data.map((d) => (
					<TypeDefaultEditor
						key={d.type}
						value={d as TypeDefault}
						onSaved={defaults.reload}
					/>
				))
			)}
		</Stack>
	);
}

function TypeDefaultEditor({
	value,
	onSaved,
}: {
	value: TypeDefault;
	onSaved: () => void;
}) {
	const save = useApiAction("backups", "set_type_default");
	const [scheduled, setScheduled] = useState(value.default_interval != null);
	const [hours, setHours] = useState(
		value.default_interval != null
			? String(Math.max(1, Math.round(value.default_interval / 3600)))
			: "6",
	);
	const [autoEnable, setAutoEnable] = useState(value.auto_enable);
	const [retention, setRetention] = useState<Retention>(
		value.default_retention ?? FLOOR_RETENTION,
	);

	const floorError = RETENTION_FIELDS.filter(
		(f) => f.floor != null && retention[f.key] < f.floor,
	).map((f) => `${f.label} must be ≥ ${f.floor}`);

	const onSave = async () => {
		await save.call({
			type: value.type,
			default_interval: scheduled ? Math.max(1, Number(hours)) * 3600 : null,
			default_retention: retention,
			auto_enable: autoEnable,
		});
		onSaved();
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack spacing={1.5}>
				<Typography sx={{ fontFamily: "monospace" }}>{value.type}</Typography>
				<FormControlLabel
					control={
						<Switch
							checked={scheduled}
							onChange={(e) => setScheduled(e.target.checked)}
							disabled={save.pending}
						/>
					}
					label={scheduled ? "Scheduled" : "Manual only"}
				/>
				{scheduled && (
					<TextField
						label="Back up every (hours)"
						type="number"
						size="small"
						value={hours}
						onChange={(e) => setHours(e.target.value)}
						disabled={save.pending}
						slotProps={{ htmlInput: { min: 1, step: 1 } }}
						sx={{ width: 200 }}
					/>
				)}
				<Stack direction={{ xs: "column", md: "row" }} spacing={1}>
					{RETENTION_FIELDS.map((f) => (
						<TextField
							key={f.key}
							label={f.label}
							type="number"
							size="small"
							value={retention[f.key]}
							onChange={(e) =>
								setRetention({ ...retention, [f.key]: Number(e.target.value) })
							}
							disabled={save.pending}
							error={f.floor != null && retention[f.key] < f.floor}
							helperText={f.floor != null ? `≥ ${f.floor}` : undefined}
							slotProps={{ htmlInput: { min: f.floor ?? 0, step: 1 } }}
							sx={{ width: 100 }}
						/>
					))}
				</Stack>
				<FormControlLabel
					control={
						<Switch
							checked={autoEnable}
							onChange={(e) => setAutoEnable(e.target.checked)}
							disabled={save.pending}
						/>
					}
					label="Auto-enable this type when a server advertises it"
				/>
				{floorError.length > 0 && (
					<Alert severity="warning">{floorError.join("; ")}</Alert>
				)}
				{save.error && <Alert severity="error">{save.error.message}</Alert>}
				<Box>
					<Button
						variant="contained"
						onClick={onSave}
						disabled={save.pending || floorError.length > 0}
					>
						{save.pending ? "Saving…" : "Save"}
					</Button>
				</Box>
			</Stack>
		</Paper>
	);
}
