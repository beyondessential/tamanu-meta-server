import {
	Alert,
	Button,
	Chip,
	LinearProgress,
	MenuItem,
	Paper,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableRow,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi, useApiAction } from "../api";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { usePageTitle } from "../hooks/usePageTitle";

/// Where every deployment is going. A group with no plan is listed too: one
/// several minors behind with nothing recorded is what this view exists to
/// surface.
// spec: UPG#the-dashboard
export default function Upgrades() {
	usePageTitle("Upgrades");
	const isAdmin = useIsAdmin() === true;
	const [tick, setTick] = useState(0);
	const fleet = useApi("upgrade_plans", "fleet", {}, [tick]);

	if (fleet.status === "loading" || fleet.status === "idle") {
		return <LinearProgress />;
	}
	if (fleet.status === "error") {
		return <Alert severity="error">{fleet.error.message}</Alert>;
	}

	const planned = fleet.data.filter((row) => row.plan);
	const unplanned = fleet.data.filter((row) => !row.plan);

	return (
		<Stack spacing={2}>
			<Typography variant="h4" component="h1">
				Upgrades
			</Typography>

			{isAdmin && (
				<RecordPlan
					groups={fleet.data.map((row) => ({
						id: row.group_id,
						name: row.group_name,
					}))}
					onRecorded={() => setTick((t) => t + 1)}
				/>
			)}

			<Paper variant="outlined" sx={{ p: 2 }} data-testid="planned-upgrades">
				<Typography variant="h6" component="h2" gutterBottom>
					Planned
				</Typography>
				{planned.length === 0 ? (
					<Typography variant="body2" color="text.secondary">
						No deployment has a recorded plan.
					</Typography>
				) : (
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell>Deployment</TableCell>
								<TableCell>Running</TableCell>
								<TableCell>Going to</TableCell>
								<TableCell>Data survives it</TableCell>
								<TableCell>Planned for</TableCell>
								<TableCell>Note</TableCell>
							</TableRow>
						</TableHead>
						<TableBody>
							{planned.map((row) => (
								<TableRow key={row.group_id} data-testid="planned-upgrade-row">
									<TableCell>
										<RouterLink to={`/servers/groups/${row.group_id}`}>
											{row.group_name}
										</RouterLink>
									</TableCell>
									<TableCell>{row.current_version ?? "unknown"}</TableCell>
									<TableCell>{row.target_version}</TableCell>
									<TableCell>
										<VerdictChip verdict={row.verdict} />
									</TableCell>
									<TableCell>
										<PlannedFor
											date={row.plan?.planned_for ?? null}
											late={row.late}
										/>
									</TableCell>
									<TableCell>{row.plan?.note ?? ""}</TableCell>
								</TableRow>
							))}
						</TableBody>
					</Table>
				)}
			</Paper>

			<Paper variant="outlined" sx={{ p: 2 }} data-testid="unplanned-upgrades">
				<Stack direction="row" spacing={1} sx={{ mb: 1, alignItems: "baseline" }}>
					<Typography variant="h6" component="h2">
						No plan recorded
					</Typography>
					<Typography variant="body2" color="text.secondary">
						pre-upgrade testing aims at the newest version for these
					</Typography>
				</Stack>
				{unplanned.length === 0 ? (
					<Typography variant="body2" color="text.secondary">
						Every deployment has a plan.
					</Typography>
				) : (
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell>Deployment</TableCell>
								<TableCell>Running</TableCell>
							</TableRow>
						</TableHead>
						<TableBody>
							{unplanned.map((row) => (
								<TableRow key={row.group_id} data-testid="unplanned-upgrade-row">
									<TableCell>
										<RouterLink to={`/servers/groups/${row.group_id}`}>
											{row.group_name}
										</RouterLink>
									</TableCell>
									<TableCell>{row.current_version ?? "unknown"}</TableCell>
								</TableRow>
							))}
						</TableBody>
					</Table>
				)}
			</Paper>
		</Stack>
	);
}

/// Whether the deployment's own data survives the planned version, rolled up
/// from its servers. Pairing it with the plan is the point of this view.
function VerdictChip({ verdict }: { verdict: string | null | undefined }) {
	if (verdict === "passed") {
		return <Chip size="small" color="success" label="passed" />;
	}
	if (verdict === "failed") {
		return (
			<Tooltip title="a server's data broke the migrations; the version is held back">
				<Chip size="small" color="warning" label="failed" />
			</Tooltip>
		);
	}
	return <Chip size="small" variant="outlined" label="not yet tested" />;
}

function PlannedFor({ date, late }: { date: string | null; late: boolean }) {
	if (!date) {
		return (
			<Typography variant="body2" color="text.secondary">
				no date
			</Typography>
		);
	}
	if (!late) {
		return <>{date}</>;
	}
	return (
		<Tooltip title="the planned day has passed and the deployment has not moved">
			<Chip size="small" color="warning" variant="outlined" label={`${date} (late)`} />
		</Tooltip>
	);
}

/// Record where a deployment is going. The version picker offers only valid
/// targets, so the operator cannot pick one the API would refuse.
// spec: UPG#a-plan
function RecordPlan({
	groups,
	onRecorded,
}: {
	groups: Array<{ id: string; name: string }>;
	onRecorded: () => void;
}) {
	const [groupId, setGroupId] = useState("");
	const [versionId, setVersionId] = useState("");
	const [plannedFor, setPlannedFor] = useState("");
	const [note, setNote] = useState("");
	const record = useApiAction("upgrade_plans", "record");
	const targets = useApi(
		"upgrade_plans",
		"targets",
		groupId ? { group_id: groupId } : undefined,
		[groupId],
	);

	const options = targets.status === "ok" ? targets.data : [];

	const submit = async () => {
		if (!groupId || !versionId) return;
		await record.call({
			group_id: groupId,
			target_version_id: versionId,
			planned_for: plannedFor || null,
			note: note || null,
		});
		setVersionId("");
		setPlannedFor("");
		setNote("");
		onRecorded();
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }} data-testid="record-plan">
			<Typography variant="h6" component="h2" gutterBottom>
				Record a plan
			</Typography>
			<Stack direction="row" spacing={1} sx={{ alignItems: "flex-start" }}>
				<TextField
					select
					size="small"
					label="Deployment"
					value={groupId}
					onChange={(e) => {
						setGroupId(e.target.value);
						setVersionId("");
					}}
					sx={{ minWidth: 180 }}
				>
					{groups.map((group) => (
						<MenuItem key={group.id} value={group.id}>
							{group.name}
						</MenuItem>
					))}
				</TextField>
				<TextField
					select
					size="small"
					label="Going to"
					value={versionId}
					disabled={!groupId || options.length === 0}
					onChange={(e) => setVersionId(e.target.value)}
					sx={{ minWidth: 140 }}
					helperText={
						groupId && options.length === 0
							? "already on the newest"
							: undefined
					}
				>
					{options.map((option) => (
						<MenuItem key={option.id} value={option.id}>
							{option.version}
						</MenuItem>
					))}
				</TextField>
				<TextField
					size="small"
					type="date"
					label="Planned for"
					value={plannedFor}
					onChange={(e) => setPlannedFor(e.target.value)}
					slotProps={{ inputLabel: { shrink: true } }}
				/>
				<TextField
					size="small"
					label="Note"
					value={note}
					onChange={(e) => setNote(e.target.value)}
					sx={{ flex: 1 }}
				/>
				<Button
					variant="contained"
					disabled={!groupId || !versionId || record.pending}
					onClick={submit}
					sx={{ mt: 0.25 }}
				>
					Record
				</Button>
			</Stack>
			{record.error && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{record.error.message}
				</Alert>
			)}
		</Paper>
	);
}
