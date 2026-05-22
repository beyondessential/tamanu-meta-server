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
import TagsEditor from "../components/TagsEditor";
import { usePageTitle } from "../hooks/usePageTitle";
import type {
	ServerGroup,
	ServerInfo,
	ServerKind,
	ServerRank,
	TagMap,
} from "../types";

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
	usePageTitle("Edit server");
	const info = useApi(
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

function EditForm({ info }: { info: ServerInfo }) {
	const navigate = useNavigate();
	const action = useApiAction("servers", "update");

	const [name, setName] = useState(info.name ?? "");
	const [host, setHost] = useState(info.host);
	const [kind, setKind] = useState<ServerKind>(info.kind);
	const [rank, setRank] = useState<ServerRank | "">(info.rank ?? "");
	const [listed, setListed] = useState(info.listed);
	// Threshold in seconds; 0 (or negative) means alerting is disabled. The
	// UI works in minutes for the operator and the empty string represents
	// "disabled" — converted back to seconds at submit time.
	const [alertWhenDownEnabled, setAlertWhenDownEnabled] = useState(
		info.alert_when_down_for > 0,
	);
	const [alertWhenDownMinutes, setAlertWhenDownMinutes] = useState<string>(
		info.alert_when_down_for > 0
			? Math.round(info.alert_when_down_for / 60).toString()
			: "10",
	);
	const [groupId, setGroupId] = useState<string | null>(info.group_id);
	const [deviceId, setDeviceId] = useState<string>(info.device_id ?? "");
	const [cloud, setCloud] = useState<"" | "true" | "false">(
		info.cloud == null ? "" : info.cloud ? "true" : "false",
	);
	const [lat, setLat] = useState<string>(info.geolocation?.lat?.toString() ?? "");
	const [lon, setLon] = useState<string>(info.geolocation?.lon?.toString() ?? "");
	const [notes, setNotes] = useState<string>(info.notes ?? "");
	const [tags, setTags] = useState<TagMap>(info.tags ?? {});

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		const data: Record<string, unknown> = {
			name: name.trim() === "" ? null : name.trim(),
			host: host.trim(),
			kind,
			rank: rank === "" ? null : rank,
			listed,
			group_id: groupId,
			device_id: deviceId.trim() === "" ? null : deviceId.trim(),
			cloud: cloud === "" ? null : cloud === "true",
			geolocation:
				lat && lon
					? { lat: Number(lat), lon: Number(lon) }
					: null,
			alert_when_down_for: alertWhenDownEnabled
				? Math.max(0, Math.round(Number(alertWhenDownMinutes) * 60))
				: 0,
			notes,
			tags,
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

				<GroupControl
					currentGroupId={groupId}
					onChange={setGroupId}
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

				<Stack
					direction={{ xs: "column", md: "row" }}
					spacing={2}
					sx={{ alignItems: { md: "center" } }}
				>
					<FormControlLabel
						control={
							<Checkbox
								checked={alertWhenDownEnabled}
								onChange={(e) => setAlertWhenDownEnabled(e.target.checked)}
								disabled={action.pending}
							/>
						}
						label="File an issue when this server is unreachable for"
					/>
					<TextField
						label="minutes"
						type="number"
						value={alertWhenDownMinutes}
						onChange={(e) => setAlertWhenDownMinutes(e.target.value)}
						disabled={action.pending || !alertWhenDownEnabled}
						slotProps={{ htmlInput: { min: 0, step: 1 } }}
						sx={{ width: 140 }}
					/>
				</Stack>
				<Typography variant="caption" color="text.secondary">
					Raise this for flappy servers (so brief blips don't fire) or lower it
					for critical servers that should page promptly. Uncheck for test
					environments and ad-hoc demos that are expected to be down.
				</Typography>

				<TextField
					label="Notes"
					multiline
					minRows={3}
					value={notes}
					onChange={(e) => setNotes(e.target.value)}
					disabled={action.pending}
					helperText="Operator notes shown on the server's detail page. Plain text."
				/>

				<Stack spacing={1}>
					<Typography variant="subtitle1">Tags</Typography>
					<TagsEditor value={tags} onChange={setTags} disabled={action.pending} />
				</Stack>

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

function GroupControl({
	currentGroupId,
	onChange,
	disabled,
}: {
	currentGroupId: string | null;
	onChange: (groupId: string | null) => void;
	disabled: boolean;
}) {
	const [query, setQuery] = useState("");
	const [results, setResults] = useState<ServerGroup[]>([]);
	const [loading, setLoading] = useState(false);

	// We fetch the *list* once so we have access to the names for whatever id
	// is currently selected. The search endpoint is for typeahead.
	const allGroups = useApi("server_groups", "list", {}, []);

	useEffect(() => {
		if (!query) {
			setResults([]);
			return;
		}
		let cancelled = false;
		setLoading(true);
		(async () => {
			try {
				const found = await callApi("server_groups", "search", { query });
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
	}, [query]);

	const currentValue = useMemo<ServerGroup | null>(() => {
		if (!currentGroupId) return null;
		if (allGroups.status === "ok") {
			return allGroups.data.find((g) => g.id === currentGroupId) ?? null;
		}
		return null;
	}, [currentGroupId, allGroups]);

	const options = useMemo<ServerGroup[]>(() => {
		if (query) return results;
		return allGroups.status === "ok" ? allGroups.data : [];
	}, [query, results, allGroups]);

	return (
		<Autocomplete<ServerGroup, false, false, false>
			disabled={disabled}
			options={options}
			value={currentValue}
			onChange={(_, v) => onChange(v?.id ?? null)}
			onInputChange={(_, v) => setQuery(v)}
			loading={loading}
			getOptionLabel={(g) => g.name}
			isOptionEqualToValue={(a, b) => a.id === b.id}
			filterOptions={(x) => x}
			renderInput={(params) => (
				<TextField
					{...params}
					label="Group"
					placeholder="Search by name, or pick from the list"
					helperText="Leave empty to keep this server ungrouped."
				/>
			)}
			renderOption={(props, group) => (
				<li {...props} key={group.id}>
					<Stack>
						<Typography variant="body2">{group.name}</Typography>
						{group.notes && (
							<Typography
								variant="caption"
								color="text.secondary"
								sx={{
									overflow: "hidden",
									textOverflow: "ellipsis",
									whiteSpace: "nowrap",
									maxWidth: "60ch",
								}}
							>
								{group.notes.split("\n")[0]}
							</Typography>
						)}
					</Stack>
				</li>
			)}
		/>
	);
}
