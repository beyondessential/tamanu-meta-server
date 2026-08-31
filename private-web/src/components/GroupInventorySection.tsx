import DeleteIcon from "@mui/icons-material/Delete";
import {
	Alert,
	Box,
	Button,
	Chip,
	LinearProgress,
	MenuItem,
	Paper,
	Stack,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { useApi, useApiAction } from "../api";
import { useIsAdmin } from "../hooks/useIsAdmin";
import type { ServerRank } from "../types";
import { SERVER_RANK_ORDER } from "../types";

type SecretVariable = {
	id: string;
	name: string;
	rank?: ServerRank | null;
	server_id?: string | null;
	set_by?: string | null;
	updated_at: string;
};

/// What a configuration run receives for each of this group's environments:
/// the servers it would act on, the address each is reached at, and the
/// variables that configure them, with the environment's values shown once
/// rather than repeated under every server.
///
/// A secret variable appears by name and never by value, and the assembled
/// inventory is admin-only, carrying those values.
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
	const isAdmin = useIsAdmin() === true;
	const [tick, setTick] = useState(0);
	const reload = () => setTick((n) => n + 1);

	const inventory = useApi(
		"inventory",
		"for_group",
		{ server_group_id: groupId, rank },
		[groupId, rank, tick],
		{ skip: !isAdmin },
	);
	const secrets = useApi(
		"inventory_secrets",
		"for_group",
		{ server_group_id: groupId },
		[groupId, tick],
	);

	const declared: SecretVariable[] =
		secrets.status === "ok" ? (secrets.data as SecretVariable[]) : [];
	const environmentSecrets = declared.filter(
		(variable) => variable.rank === rank && !variable.server_id,
	);
	const serverSecrets = (serverId: string) =>
		declared.filter((variable) => variable.server_id === serverId);

	const hosts = inventory.status === "ok" ? inventory.data.hosts : [];

	return (
		<Paper variant="outlined" sx={{ p: 2 }} data-testid={`environment-${rank}`}>
			<Typography
				variant="overline"
				color="text.secondary"
				sx={{ display: "block" }}
			>
				{rank}
			</Typography>

			{(inventory.status === "loading" || secrets.status === "loading") && (
				<LinearProgress />
			)}

			{inventory.status === "error" && (
				<Alert severity="warning" sx={{ mt: 1 }}>
					{inventory.error.message}
				</Alert>
			)}

			{!isAdmin && (
				<Alert severity="info" sx={{ mt: 1 }} data-testid="inventory-needs-admin">
					The assembled inventory carries the environment's secret values, so
					reading it needs admin access. The names set here are below.
				</Alert>
			)}

			{!isAdmin && (
				<Box sx={{ mt: 2 }}>
					<Typography variant="body2" color="text.secondary">
						Secret variables
					</Typography>
					<Secrets items={environmentSecrets} />
				</Box>
			)}

			{isAdmin && inventory.status === "ok" && (
				<Stack spacing={2} sx={{ mt: 1 }}>
					<Box>
						<Typography variant="body2" color="text.secondary" gutterBottom>
							Environment variables, carried by every server below
						</Typography>
						<Vars
							vars={inventory.data.vars}
							secret={inventory.data.secret_vars}
						/>
						<Secrets
							items={environmentSecrets}
							onRemove={reload}
							scope={{ server_group_id: groupId, rank }}
						/>
					</Box>

					{hosts.map((host) => (
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
								secret={host.secret_vars}
								empty="Sets nothing of its own"
							/>
							<Secrets
								items={serverSecrets(host.id)}
								onRemove={reload}
								scope={{ server_id: host.id }}
							/>
						</Box>
					))}

					<SetSecret
						groupId={groupId}
						rank={rank}
						hosts={hosts}
						onSet={reload}
					/>
				</Stack>
			)}
		</Paper>
	);
}

/// Variables as chips. Where a set of environment values is passed, a key
/// present in both is marked as overriding rather than adding, since that is
/// the case an operator chasing a value that isn't taking effect is looking
/// for. A secret's value is never among them.
function Vars({
	vars,
	overrides,
	secret = [],
	empty = "None set",
}: {
	vars: Record<string, unknown>;
	overrides?: Record<string, unknown>;
	secret?: ReadonlyArray<string>;
	empty?: string;
}) {
	const hidden = new Set(secret);
	const keys = Object.keys(vars)
		.filter((key) => !hidden.has(key))
		.sort();
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
				const overriding =
					overrides !== undefined && key in overrides && !hidden.has(key);
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
					<Tooltip key={key} title="Overrides the environment's value">
						{chip}
					</Tooltip>
				) : (
					chip
				);
			})}
		</Stack>
	);
}

/// Secret variables by name, with where each is set and when it last changed.
/// Never a value: what a run receives is served to the run.
function Secrets({
	items,
	scope,
	onRemove,
}: {
	items: ReadonlyArray<SecretVariable>;
	scope?: Record<string, unknown>;
	onRemove?: () => void;
}) {
	const remove = useApiAction("inventory_secrets", "remove");
	if (items.length === 0) return null;

	return (
		<>
			<Stack
				direction="row"
				spacing={0.5}
				useFlexGap
				sx={{ flexWrap: "wrap", mt: 0.5 }}
			>
			{items.map((variable) => (
				<Tooltip
					key={variable.id}
					title={`Set by ${variable.set_by ?? "unknown"}, ${new Date(
						variable.updated_at,
					).toLocaleString()}`}
				>
					<Chip
						size="small"
						variant="outlined"
						color="warning"
						label={`${variable.name} = secret`}
						data-testid="secret-var"
						sx={{ fontFamily: "monospace", maxWidth: "100%" }}
						onDelete={
							scope && onRemove
								? () => {
										remove
											.call({ ...scope, name: variable.name })
											.then(onRemove)
											.catch(() => {
												// Surfaced by the alert below.
											});
									}
								: undefined
						}
						deleteIcon={
							<DeleteIcon
								data-testid={`remove-${variable.name}`}
								titleAccess={`remove ${variable.name}`}
							/>
						}
					/>
				</Tooltip>
			))}
			</Stack>
			{remove.error && (
				<Alert severity="warning" sx={{ mt: 1 }}>
					{remove.error.message}
				</Alert>
			)}
		</>
	);
}

const ENVIRONMENT = "environment";

function SetSecret({
	groupId,
	rank,
	hosts,
	onSet,
}: {
	groupId: string;
	rank: ServerRank;
	hosts: ReadonlyArray<{ id: string; name: string }>;
	onSet: () => void;
}) {
	const [where, setWhere] = useState<string>(ENVIRONMENT);
	const [name, setName] = useState("");
	const [value, setValue] = useState("");
	const set = useApiAction("inventory_secrets", "set");

	const submit = () => {
		const scope =
			where === ENVIRONMENT
				? { server_group_id: groupId, rank }
				: { server_id: where };
		set
			.call({ ...scope, name: name.trim(), value })
			.then(() => {
				setName("");
				setValue("");
				onSet();
			})
			.catch(() => {});
	};

	return (
		<Box data-testid="set-secret">
			<Typography variant="body2" color="text.secondary" gutterBottom>
				Set a secret variable
			</Typography>
			<Stack direction="row" spacing={1} useFlexGap sx={{ flexWrap: "wrap" }}>
				<TextField
					select
					size="small"
					label="Scope"
					value={where}
					onChange={(event) => setWhere(event.target.value)}
					sx={{ minWidth: 180 }}
				>
					<MenuItem value={ENVIRONMENT}>Whole environment</MenuItem>
					{hosts.map((host) => (
						<MenuItem key={host.id} value={host.id}>
							{host.name}
						</MenuItem>
					))}
				</TextField>
				<TextField
					size="small"
					label="Name"
					value={name}
					onChange={(event) => setName(event.target.value)}
				/>
				<TextField
					size="small"
					label="Value"
					type="password"
					value={value}
					onChange={(event) => setValue(event.target.value)}
				/>
				<Button
					variant="outlined"
					onClick={submit}
					disabled={set.pending || name.trim() === "" || value === ""}
				>
					Set
				</Button>
			</Stack>
			{set.error && (
				<Alert severity="warning" sx={{ mt: 1 }}>
					{set.error.message}
				</Alert>
			)}
		</Box>
	);
}

function format(value: unknown): string {
	return typeof value === "string" ? value : JSON.stringify(value);
}
