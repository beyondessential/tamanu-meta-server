import {
	Alert,
	Box,
	Button,
	Chip,
	LinearProgress,
	Paper,
	Stack,
	Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import ArchiveIcon from "@mui/icons-material/ArchiveOutlined";
import EditIcon from "@mui/icons-material/Edit";
import RestoreIcon from "@mui/icons-material/RestoreFromTrash";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { Link as RouterLink, useNavigate, useParams } from "react-router-dom";
import ServerShorty from "../components/ServerShorty";
import SilencedRefsSection from "../components/SilencedRefsSection";
import TimeAgo from "../components/TimeAgo";
import { useApi, useApiAction } from "../api";
import { useIsNotificationHeld } from "../hooks/useIsNotificationHeld";
import { usePageTitle } from "../hooks/usePageTitle";
import {
	SERVER_RANK_ORDER,
	compareServersByRankThenKind,
	type IncidentData,
	type ServerInfo,
	type ServerRank,
} from "../types";

export default function GroupDetail() {
	const { id = "" } = useParams<{ id: string }>();
	const navigate = useNavigate();
	const detail = useApi("server_groups", "get", { server_group_id: id }, [id]);
	const isAdmin = useApi("commons", "is_current_user_admin");
	const archive = useApiAction("server_groups", "delete");
	// Only the currently-open incident matters for the active-incident
	// section; closed ones live behind the /incidents filter route.
	const activeIncidents = useApi(
		"incidents",
		"list_for_group",
		{ server_group_id: id, include_closed: false, limit: 1 },
		[id],
	);
	usePageTitle(detail.status === "ok" ? detail.data.group.name : "Group");

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <Alert severity="error">{detail.error.message}</Alert>;
	}

	const { group, servers } = detail.data;
	const admin = isAdmin.status === "ok" && isAdmin.data;
	const tagEntries = Object.entries(group.tags ?? {});
	const openIncident =
		activeIncidents.status === "ok" && activeIncidents.data.length > 0
			? activeIncidents.data[0]
			: null;

	const onArchive = async () => {
		if (
			!confirm(
				`Archive group "${group.name}"? It's hidden from listings but can be restored from the Archived tab.`,
			)
		)
			return;
		try {
			await archive.call({ server_group_id: group.id });
			navigate("/servers");
		} catch {
			/* surfaced via archive.error */
		}
	};

	return (
		<Stack spacing={3}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h4" component="h1">
					{group.name}
				</Typography>
				{admin && (
					<Stack direction="row" spacing={1}>
						<Button
							component={RouterLink}
							to={`/groups/${group.id}/servers/new`}
							variant="contained"
							startIcon={<AddIcon />}
						>
							Add server
						</Button>
						<Button
							component={RouterLink}
							to={`/groups/${group.id}/edit`}
							variant="outlined"
							startIcon={<EditIcon />}
						>
							Edit
						</Button>
						{servers.length === 0 && !group.deleted_at && (
							<Button
								variant="outlined"
								color="error"
								startIcon={<ArchiveIcon />}
								onClick={onArchive}
								disabled={archive.pending}
							>
								Archive
							</Button>
						)}
					</Stack>
				)}
			</Stack>

			{archive.error && (
				<Alert severity="error">{archive.error.message}</Alert>
			)}

			{group.deleted_at && (
				<ArchivedGroupBanner
					groupId={group.id}
					isAdmin={admin}
					onRestored={detail.reload}
				/>
			)}

			{openIncident && <ActiveIncidentCard incident={openIncident} />}

			{group.notes && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Typography variant="h6" component="h2" gutterBottom>
						Notes
					</Typography>
					<Typography variant="body2" sx={{ whiteSpace: "pre-wrap" }}>
						{group.notes}
					</Typography>
				</Paper>
			)}

			{tagEntries.length > 0 && (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Typography variant="h6" component="h2" gutterBottom>
						Tags
					</Typography>
					<Box sx={{ display: "flex", flexWrap: "wrap", gap: 0.5 }}>
						{tagEntries.map(([k, v]) => (
							<Chip
								key={k}
								size="small"
								variant="outlined"
								label={`${k}=${v}`}
							/>
						))}
					</Box>
				</Paper>
			)}

			<Box>
				<Typography variant="h5" component="h2" gutterBottom>
					Servers ({servers.length})
				</Typography>
				{servers.length === 0 ? (
					<Alert severity="info">
						No servers in this group yet. Use “Add server” above to enroll
						one into this group.
					</Alert>
				) : (
					<Stack spacing={2}>
						{groupServersByRank(servers).map(([rank, members]) => (
							<Box key={rank ?? "_unranked"}>
								{rank && (
									<Typography
										variant="overline"
										color="text.secondary"
										sx={{ display: "block", mb: 0.5 }}
									>
										{rank}
									</Typography>
								)}
								<Stack spacing={1}>
									{members.map((s) => (
										<ServerShorty key={s.id} server={s} />
									))}
								</Stack>
							</Box>
						))}
					</Stack>
				)}
			</Box>

			<SilencedRefsSection scope="group" id={group.id} />
		</Stack>
	);
}

function ActiveIncidentCard({ incident }: { incident: IncidentData }) {
	const held = useIsNotificationHeld(incident.notification_held_until);
	return (
		<Paper
			variant="outlined"
			sx={{
				p: 2,
				borderColor: held ? "warning.main" : "error.main",
				borderWidth: 2,
			}}
		>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Stack direction="row" spacing={1.5} sx={{ alignItems: "center" }}>
					<WarningAmberIcon color={held ? "warning" : "error"} />
					<Box>
						<Typography variant="h6" component="h2">
							Active incident
							<Box
								component="span"
								sx={{
									ml: 1,
									fontFamily: "monospace",
									color: "text.secondary",
									fontWeight: "normal",
									fontSize: "0.85em",
								}}
							>
								{incident.id.slice(0, 8)}
							</Box>
						</Typography>
						<Typography variant="body2" color="text.secondary">
							opened <TimeAgo timestamp={incident.opened_at} />
						</Typography>
						{held && incident.notification_held_until && (
							<Typography variant="body2" sx={{ color: "warning.main" }}>
								Holding; posting{" "}
								<TimeAgo timestamp={incident.notification_held_until} />
							</Typography>
						)}
					</Box>
				</Stack>
				<Button
					component={RouterLink}
					to={`/incidents/${incident.id}`}
					variant="outlined"
					color={held ? "warning" : "error"}
				>
					Open
				</Button>
			</Stack>
		</Paper>
	);
}

/// Group a flat server list into rank buckets in display order, with
/// each bucket internally sorted by kind (centrals first) then name.
/// Servers without a rank land in a trailing `null` bucket.
function groupServersByRank(
	servers: ServerInfo[],
): Array<[ServerRank | null, ServerInfo[]]> {
	const buckets = new Map<ServerRank | null, ServerInfo[]>();
	for (const s of servers) {
		const rank = s.rank ?? null;
		const list = buckets.get(rank);
		if (list) list.push(s);
		else buckets.set(rank, [s]);
	}
	const order: Array<ServerRank | null> = [...SERVER_RANK_ORDER, null];
	const result: Array<[ServerRank | null, ServerInfo[]]> = [];
	for (const rank of order) {
		const list = buckets.get(rank);
		if (list && list.length > 0) {
			list.sort(compareServersByRankThenKind);
			result.push([rank, list]);
		}
	}
	return result;
}

function ArchivedGroupBanner({
	groupId,
	isAdmin,
	onRestored,
}: {
	groupId: string;
	isAdmin: boolean;
	onRestored: () => void;
}) {
	const action = useApiAction("server_groups", "restore");
	const onRestore = async () => {
		try {
			await action.call({ server_group_id: groupId });
			onRestored();
		} catch {
			/* surfaced via action.error */
		}
	};
	return (
		<Alert
			severity="warning"
			action={
				isAdmin ? (
					<Button
						color="inherit"
						size="small"
						startIcon={<RestoreIcon />}
						onClick={onRestore}
						disabled={action.pending}
					>
						{action.pending ? "Restoring…" : "Restore"}
					</Button>
				) : undefined
			}
		>
			This group is archived. Restore it to bring it back to the listings.
			{action.error && <Box sx={{ mt: 1 }}>{action.error.message}</Box>}
		</Alert>
	);
}
