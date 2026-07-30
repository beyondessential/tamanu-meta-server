import {
	Alert,
	Chip,
	LinearProgress,
	Paper,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableRow,
	Tooltip,
	Typography,
} from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

/// Where every deployment is going. A group with no plan is listed too: one
/// several minors behind with nothing recorded is what this view exists to
/// surface.
// spec: UPG#the-dashboard
export default function Upgrades() {
	usePageTitle("Upgrades");
	const fleet = useApi("upgrade_plans", "fleet", {}, []);

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
