import {
	Alert,
	Box,
	Chip,
	LinearProgress,
	Paper,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import { useApi } from "../api";
import type { ServerRank } from "../types";
import { SERVER_RANK_ORDER } from "../types";

/// What a configuration run receives for each of this group's environments:
/// the servers it would act on, the address each is reached at, and the
/// variables that configure them, with the group's values shown once rather
/// than repeated under every server.
// spec: INV#presentation
export default function GroupInventorySection({
	groupId,
	servers,
}: {
	groupId: string;
	servers: ReadonlyArray<{ rank?: ServerRank | null }>;
}) {
	// A server carrying no rank sits in the default-rank environment, so the
	// ranks here are the effective ones rather than the stored ones.
	const ranks = SERVER_RANK_ORDER.filter((rank) =>
		servers.some((server) => (server.rank ?? "dev") === rank),
	);

	return (
		<Box data-testid="group-inventory">
			<Typography variant="h6" gutterBottom>
				Inventory
			</Typography>
			{ranks.length === 0 ? (
				<Paper variant="outlined" sx={{ p: 2 }}>
					<Typography variant="body2" color="text.secondary">
						No live servers, so there is no environment to configure.
					</Typography>
				</Paper>
			) : (
				<Stack spacing={2}>
					{ranks.map((rank) => (
						<EnvironmentInventory key={rank} groupId={groupId} rank={rank} />
					))}
				</Stack>
			)}
		</Box>
	);
}

function EnvironmentInventory({
	groupId,
	rank,
}: {
	groupId: string;
	rank: ServerRank;
}) {
	const inventory = useApi(
		"inventory",
		"for_group",
		{ server_group_id: groupId, rank },
		[groupId, rank],
	);

	return (
		<Paper variant="outlined" sx={{ p: 2 }} data-testid={`environment-${rank}`}>
			<Typography
				variant="overline"
				color="text.secondary"
				sx={{ display: "block" }}
			>
				{rank}
			</Typography>

			{(inventory.status === "loading" || inventory.status === "idle") && (
				<LinearProgress />
			)}

			{inventory.status === "error" && (
				<Alert severity="warning" sx={{ mt: 1 }}>
					{inventory.error.message}
				</Alert>
			)}

			{inventory.status === "ok" && (
				<Stack spacing={2} sx={{ mt: 1 }}>
					<Box>
						<Typography variant="body2" color="text.secondary" gutterBottom>
							Group variables, carried by every server below
						</Typography>
						<Vars vars={inventory.data.vars} />
					</Box>

					{inventory.data.hosts.map((host) => (
						<Box key={host.id} data-testid="inventory-server">
							<Stack
								direction="row"
								spacing={1}
								sx={{ alignItems: "baseline", flexWrap: "wrap" }}
							>
								<Typography variant="subtitle2">{host.name}</Typography>
								<Chip size="small" label={host.kind} />
								<Typography
									variant="body2"
									color="text.secondary"
									sx={{ fontFamily: "monospace" }}
								>
									{host.address ?? "no address"}
								</Typography>
							</Stack>
							<Vars
								vars={host.own_vars}
								overrides={inventory.data.vars}
								empty="Sets nothing of its own"
							/>
						</Box>
					))}
				</Stack>
			)}
		</Paper>
	);
}

/// Variables as chips. Where a set of group values is passed, a key present in
/// both is marked as overriding rather than adding, since that is the case an
/// operator chasing a value that isn't taking effect is looking for.
function Vars({
	vars,
	overrides,
	empty = "None set",
}: {
	vars: Record<string, unknown>;
	overrides?: Record<string, unknown>;
	empty?: string;
}) {
	const keys = Object.keys(vars).sort();
	if (keys.length === 0) {
		return (
			<Typography variant="body2" color="text.secondary">
				{empty}
			</Typography>
		);
	}
	return (
		<Stack
			direction="row"
			spacing={0.5}
			useFlexGap
			sx={{ flexWrap: "wrap", mt: 0.5 }}
		>
			{keys.map((key) => {
				const overriding = overrides !== undefined && key in overrides;
				const chip = (
					<Chip
						key={key}
						size="small"
						variant={overriding ? "filled" : "outlined"}
						label={`${key} = ${format(vars[key])}`}
						data-testid={overriding ? "overriding-var" : "var"}
						sx={{ fontFamily: "monospace", maxWidth: "100%" }}
					/>
				);
				return overriding ? (
					<Tooltip key={key} title="Overrides the group's value">
						{chip}
					</Tooltip>
				) : (
					chip
				);
			})}
		</Stack>
	);
}

function format(value: unknown): string {
	return typeof value === "string" ? value : JSON.stringify(value);
}
