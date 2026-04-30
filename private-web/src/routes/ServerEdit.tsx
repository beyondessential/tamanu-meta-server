import {
	Alert,
	Autocomplete,
	Button,
	Checkbox,
	FormControlLabel,
	LinearProgress,
	MenuItem,
	Paper,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
import { useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { callApi, useApi, useApiAction } from "../api";
import type { ServerInfoFull, ServerKind, ServerRank } from "../types";

const RANK_OPTIONS: Array<{ value: ServerRank | ""; label: string }> = [
	{ value: "", label: "unranked" },
	{ value: "production", label: "production" },
	{ value: "clone", label: "clone" },
	{ value: "demo", label: "demo" },
	{ value: "test", label: "test" },
	{ value: "dev", label: "dev" },
];

export default function ServerEdit() {
	const { id = "" } = useParams<{ id: string }>();
	const info = useApi<ServerInfoFull>(
		"servers",
		"get_info",
		{ server_id: id },
		[id],
	);

	if (info.status === "loading" || info.status === "idle") return <LinearProgress />;
	if (info.status === "error")
		return <Alert severity="error">{info.error.message}</Alert>;
	return <EditForm info={info.data} />;
}

function EditForm({ info }: { info: ServerInfoFull }) {
	const navigate = useNavigate();
	const action = useApiAction("servers", "update");

	const [name, setName] = useState(info.name ?? "");
	const [host, setHost] = useState(info.host);
	const [kind, setKind] = useState<ServerKind>(info.kind);
	const [rank, setRank] = useState<ServerRank | "">(info.rank ?? "");
	const [listed, setListed] = useState(info.listed);
	const [parentId, setParentId] = useState<string | null>(
		info.parent_server_id,
	);
	const [deviceId, setDeviceId] = useState<string>(info.device_id ?? "");
	const [cloud, setCloud] = useState<"" | "true" | "false">(
		info.cloud == null ? "" : info.cloud ? "true" : "false",
	);
	const [lat, setLat] = useState<string>(info.geolocation?.lat?.toString() ?? "");
	const [lon, setLon] = useState<string>(info.geolocation?.lon?.toString() ?? "");

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		const data: Record<string, unknown> = {
			name: name.trim() === "" ? null : name.trim(),
			host: host.trim(),
			kind,
			rank: rank === "" ? null : rank,
			listed,
			parent_server_id: parentId,
			device_id: deviceId.trim() === "" ? null : deviceId.trim(),
			cloud: cloud === "" ? null : cloud === "true",
			geolocation:
				lat && lon
					? { lat: Number(lat), lon: Number(lon) }
					: null,
		};
		try {
			await action.call({ server_id: info.id, data });
			navigate(`/servers/${info.id}`);
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 3 }} component="form" onSubmit={onSubmit}>
			<Stack spacing={2}>
				<Typography variant="h5" component="h1">
					Edit server
				</Typography>

				<TextField
					label="Name"
					value={name}
					onChange={(e) => setName(e.target.value)}
					disabled={action.pending}
				/>
				<TextField
					label="URL"
					value={host}
					onChange={(e) => setHost(e.target.value)}
					disabled={action.pending}
					required
				/>
				<TextField
					select
					label="Kind"
					value={kind}
					onChange={(e) => setKind(e.target.value as ServerKind)}
					disabled={action.pending}
				>
					<MenuItem value="central">central</MenuItem>
					<MenuItem value="facility">facility</MenuItem>
				</TextField>
				<TextField
					select
					label="Rank"
					value={rank}
					onChange={(e) => setRank(e.target.value as ServerRank | "")}
					disabled={action.pending}
				>
					{RANK_OPTIONS.map((o) => (
						<MenuItem key={o.value} value={o.value}>
							{o.label}
						</MenuItem>
					))}
				</TextField>
				<TextField
					label="Device ID"
					placeholder="UUID"
					value={deviceId}
					onChange={(e) => setDeviceId(e.target.value)}
					disabled={action.pending}
				/>

				<ParentServerControl
					serverId={info.id}
					currentParentId={parentId}
					currentKind={kind}
					currentRank={rank === "" ? null : rank}
					onChange={setParentId}
					disabled={action.pending}
				/>

				<Stack direction={{ xs: "column", md: "row" }} spacing={2}>
					<TextField
						select
						label="Location"
						value={cloud}
						onChange={(e) => setCloud(e.target.value as "" | "true" | "false")}
						disabled={action.pending}
						sx={{ minWidth: 180 }}
					>
						<MenuItem value="">unknown</MenuItem>
						<MenuItem value="true">cloud</MenuItem>
						<MenuItem value="false">on premise</MenuItem>
					</TextField>
					<TextField
						label="Latitude"
						value={lat}
						onChange={(e) => setLat(e.target.value)}
						disabled={action.pending}
						sx={{ flex: 1 }}
					/>
					<TextField
						label="Longitude"
						value={lon}
						onChange={(e) => setLon(e.target.value)}
						disabled={action.pending}
						sx={{ flex: 1 }}
					/>
				</Stack>

				{kind === "central" && (
					<FormControlLabel
						control={
							<Checkbox
								checked={listed}
								onChange={(e) => setListed(e.target.checked)}
								disabled={action.pending}
							/>
						}
						label="Available in Tamanu Mobile app"
					/>
				)}

				{action.error && (
					<Alert severity="error">{action.error.message}</Alert>
				)}

				<Stack direction="row" spacing={1}>
					<Button
						type="submit"
						variant="contained"
						disabled={action.pending}
					>
						{action.pending ? "Saving…" : "Save"}
					</Button>
					<Button
						type="button"
						variant="outlined"
						color="error"
						onClick={() => navigate(`/servers/${info.id}`)}
						disabled={action.pending}
					>
						Cancel
					</Button>
				</Stack>
			</Stack>
		</Paper>
	);
}

function ParentServerControl({
	serverId,
	currentParentId,
	currentKind,
	currentRank,
	onChange,
	disabled,
}: {
	serverId: string;
	currentParentId: string | null;
	currentKind: ServerKind;
	currentRank: ServerRank | null;
	onChange: (parentId: string | null) => void;
	disabled: boolean;
}) {
	const [query, setQuery] = useState("");
	const [results, setResults] = useState<ServerInfoFull[]>([]);
	const [loading, setLoading] = useState(false);

	const currentInfo = useApi<ServerInfoFull>(
		"servers",
		"get_info",
		{ server_id: currentParentId ?? "" },
		[currentParentId ?? ""],
	);

	useEffect(() => {
		if (!query) {
			setResults([]);
			return;
		}
		let cancelled = false;
		setLoading(true);
		(async () => {
			try {
				const found = await callApi<ServerInfoFull[]>(
					"servers",
					"search_parent",
					{
						query,
						current_server_id: serverId,
						current_rank: currentRank,
						current_kind: currentKind,
					},
				);
				if (!cancelled) setResults(found);
			} catch {
				if (!cancelled) setResults([]);
			} finally {
				if (!cancelled) setLoading(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [query, serverId, currentRank, currentKind]);

	const currentValue = useMemo(() => {
		if (!currentParentId) return null;
		if (currentInfo.status === "ok") return currentInfo.data;
		return { id: currentParentId } as Partial<ServerInfoFull> as ServerInfoFull;
	}, [currentParentId, currentInfo]);

	return (
		<Autocomplete<ServerInfoFull, false, false, false>
			disabled={disabled}
			options={results}
			value={currentValue}
			onChange={(_, v) => onChange(v?.id ?? null)}
			onInputChange={(_, v) => setQuery(v)}
			loading={loading}
			getOptionLabel={(s) => s.name ?? s.host ?? s.id}
			isOptionEqualToValue={(a, b) => a.id === b.id}
			filterOptions={(x) => x}
			renderInput={(params) => (
				<TextField
					{...params}
					label="Parent server"
					placeholder="Search by name or host, or paste a UUID"
				/>
			)}
			renderOption={(props, server) => (
				<li {...props} key={server.id}>
					<Stack>
						<Typography variant="body2">
							{server.name ?? server.host}
						</Typography>
						<Typography variant="caption" color="text.secondary">
							{server.host} • {server.kind}
							{server.rank ? ` • ${server.rank}` : " • unranked"}
						</Typography>
					</Stack>
				</li>
			)}
		/>
	);
}
