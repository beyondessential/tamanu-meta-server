import {
	Alert,
	Autocomplete,
	Button,
	Checkbox,
	FormControlLabel,
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
	ServerKind,
	ServerRank,
	TagMap,
	TailnetLiveInfo,
} from "../types";

const RANK_OPTIONS: Array<{ value: ServerRank | ""; label: string }> = [
	{ value: "", label: "unranked" },
	{ value: "production", label: "production" },
	{ value: "clone", label: "clone" },
	{ value: "demo", label: "demo" },
	{ value: "test", label: "test" },
	{ value: "dev", label: "dev" },
];

/// Operator-first server creation, reachable at `/groups/:id/servers/new` with
/// the group preselected. A group is required — servers are always grouped.
export default function ServerCreate() {
	usePageTitle("Add server");
	const navigate = useNavigate();
	// When mounted under `/groups/:id/servers/new`, the route param is the
	// group to default-select.
	const { id: presetGroupId } = useParams<{ id?: string }>();
	const action = useApiAction("servers", "create");

	const [name, setName] = useState("");
	const [host, setHost] = useState("");
	const [kind, setKind] = useState<ServerKind>("facility");
	const [rank, setRank] = useState<ServerRank | "">("");
	const [publicName, setPublicName] = useState("");
	const [isMonitored, setIsMonitored] = useState(true);
	const [alertWhenDownMinutes, setAlertWhenDownMinutes] = useState("10");
	const [groupId, setGroupId] = useState<string | null>(presetGroupId ?? null);
	const [tailscaleIdentifier, setTailscaleIdentifier] = useState("");
	const [cloud, setCloud] = useState<"" | "true" | "false">("");
	const [lat, setLat] = useState("");
	const [lon, setLon] = useState("");
	const [notes, setNotes] = useState("");
	const [tags, setTags] = useState<TagMap>({});

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		if (!groupId) return; // a group is required
		const data: Record<string, unknown> = {
			name: name.trim() === "" ? null : name.trim(),
			host: host.trim(),
			kind,
			rank: rank === "" ? null : rank,
			public_name: publicName.trim() === "" ? null : publicName.trim(),
			group_id: groupId,
			tailscale_identifier:
				tailscaleIdentifier.trim() === "" ? null : tailscaleIdentifier.trim(),
			cloud: cloud === "" ? null : cloud === "true",
			geolocation:
				lat && lon ? { lat: Number(lat), lon: Number(lon) } : null,
			is_monitored: isMonitored,
			alert_when_down_for: Math.max(
				60,
				Math.round(Number(alertWhenDownMinutes) * 60),
			),
			notes,
			tags,
		};
		try {
			const serverId = await action.call(data);
			navigate(`/servers/${serverId}`);
		} catch {
			/* surfaced via action.error */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 3 }} component="form" onSubmit={onSubmit}>
			<Stack spacing={2}>
				<Typography variant="h5" component="h1">
					Add server
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

				<TailscaleIdentityField
					value={tailscaleIdentifier}
					onChange={setTailscaleIdentifier}
					disabled={action.pending}
				/>

				<GroupControl
					currentGroupId={groupId}
					onChange={setGroupId}
					disabled={action.pending}
					required
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
					<TextField
						label="Name in Tamanu Mobile app"
						value={publicName}
						onChange={(e) => setPublicName(e.target.value)}
						disabled={action.pending}
						helperText="Leave empty to hide this server from the public mobile-app list."
					/>
				)}

				<FormControlLabel
					control={
						<Checkbox
							checked={isMonitored}
							onChange={(e) => setIsMonitored(e.target.checked)}
							disabled={action.pending}
						/>
					}
					label="Monitor this server"
				/>
				<Typography variant="caption" color="text.secondary">
					When off, canopy stops watching this server: reachability sweeps
					skip it and its events/issues no longer trigger or join incidents.
					Use this for test environments and ad-hoc demos that are expected
					to be down.
				</Typography>

				<Stack
					direction={{ xs: "column", md: "row" }}
					spacing={2}
					sx={{ alignItems: { md: "center" } }}
				>
					<Typography variant="body2">
						File an issue when this server is unreachable for
					</Typography>
					<TextField
						label="minutes"
						type="number"
						value={alertWhenDownMinutes}
						onChange={(e) => setAlertWhenDownMinutes(e.target.value)}
						disabled={action.pending || !isMonitored}
						slotProps={{ htmlInput: { min: 1, step: 1 } }}
						sx={{ width: 140 }}
					/>
				</Stack>

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
						disabled={action.pending || !groupId}
					>
						{action.pending ? "Creating…" : "Create server"}
					</Button>
					<Button
						type="button"
						variant="outlined"
						color="error"
						onClick={() => navigate(-1)}
						disabled={action.pending}
					>
						Cancel
					</Button>
				</Stack>
			</Stack>
		</Paper>
	);
}

/// Optional Tailscale identity field with a debounced live preview from
/// `devices.resolve_tailnet_identifier`. The raw value is passed to
/// `servers.create` as `tailscale_identifier`.
function TailscaleIdentityField({
	value,
	onChange,
	disabled,
}: {
	value: string;
	onChange: (value: string) => void;
	disabled: boolean;
}) {
	const [preview, setPreview] = useState<TailnetLiveInfo | null>(null);
	const [previewError, setPreviewError] = useState<string | null>(null);
	const [previewLoading, setPreviewLoading] = useState(false);

	useEffect(() => {
		const trimmed = value.trim();
		if (trimmed === "") {
			setPreview(null);
			setPreviewError(null);
			return;
		}
		let cancelled = false;
		setPreviewLoading(true);
		const handle = setTimeout(async () => {
			try {
				const r = await callApi("devices", "resolve_tailnet_identifier", {
					identifier: trimmed,
				});
				if (cancelled) return;
				setPreview(r.matched);
				setPreviewError(
					r.matched ? null : "No tailnet node matches that identifier.",
				);
			} catch (err) {
				if (cancelled) return;
				setPreview(null);
				setPreviewError(err instanceof Error ? err.message : String(err));
			} finally {
				if (!cancelled) setPreviewLoading(false);
			}
		}, 250);
		return () => {
			cancelled = true;
			clearTimeout(handle);
		};
	}, [value]);

	return (
		<Stack spacing={1}>
			<TextField
				label="Tailscale identity"
				placeholder="100.64.0.42 / nodekey:… / device.example.ts.net"
				value={value}
				onChange={(e) => onChange(e.target.value)}
				disabled={disabled}
				helperText="Bind this server to a tailnet node up front. Leave empty to enroll it later."
			/>
			{previewLoading && (
				<Typography variant="caption" color="text.secondary">
					Resolving…
				</Typography>
			)}
			{preview && (
				<Paper variant="outlined" sx={{ p: 1.5 }}>
					<Stack spacing={0.5}>
						<Typography variant="caption" color="text.secondary">
							Resolves to
						</Typography>
						<Typography variant="body2">{preview.display_name}</Typography>
						<Typography
							variant="body2"
							color="text.secondary"
							sx={{ fontFamily: "monospace" }}
						>
							{preview.node_id}
						</Typography>
						<Typography variant="body2" color="text.secondary">
							{preview.addresses.join(", ")}
						</Typography>
					</Stack>
				</Paper>
			)}
			{previewError && value.trim() !== "" && (
				<Alert severity="info">{previewError}</Alert>
			)}
		</Stack>
	);
}

function GroupControl({
	currentGroupId,
	onChange,
	disabled,
	required = false,
}: {
	currentGroupId: string | null;
	onChange: (groupId: string | null) => void;
	disabled: boolean;
	required?: boolean;
}) {
	const [query, setQuery] = useState("");
	const [results, setResults] = useState<ServerGroup[]>([]);
	const [loading, setLoading] = useState(false);

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
			renderInput={(params) => {
				const missing = required && !currentValue;
				return (
					<TextField
						{...params}
						label="Group"
						required={required}
						error={missing}
						placeholder="Search by name, or pick from the list"
						helperText={
							missing
								? "Required — every server belongs to a group."
								: "The group this server belongs to."
						}
					/>
				);
			}}
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
