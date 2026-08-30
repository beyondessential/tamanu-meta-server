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
import {
	useProductCaps,
	useProductKinds,
	useProducts,
} from "../hooks/useProducts";
import { PRODUCT_LABELS, REACHABILITY_CHECK } from "../types";
import type {
	Product,
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
	// The unreachability toggle *is* the server-scoped silence on canopy's
	// reachability check, so the form can't render until we know whether one
	// exists — a wrong initial value would be written back on save.
	const silences = useApi(
		"silenced_refs",
		"list_for_server",
		{ server_id: id },
		[id],
	);

	if (
		info.status === "loading" ||
		info.status === "idle" ||
		silences.status === "loading" ||
		silences.status === "idle"
	)
		return <LinearProgress />;
	if (info.status === "error")
		return <Alert severity="error">{info.error.message}</Alert>;
	if (silences.status === "error")
		return <Alert severity="error">{silences.error.message}</Alert>;
	return (
		<EditForm
			info={info.data}
			reachabilitySilenced={silences.data.some(
				(s) =>
					s.source === REACHABILITY_CHECK.source &&
					s.ref === REACHABILITY_CHECK.ref,
			)}
		/>
	);
}

function EditForm({
	info,
	reachabilitySilenced,
}: {
	info: ServerInfo;
	reachabilitySilenced: boolean;
}) {
	const navigate = useNavigate();
	const action = useApiAction("servers", "update");
	const silence = useApiAction("silenced_refs", "silence_server");
	const unsilence = useApiAction("silenced_refs", "unsilence_server");

	const [name, setName] = useState(info.name ?? "");
	const [host, setHost] = useState(info.host ?? "");
	const [product, setProduct] = useState<Product>(info.product);
	const [kind, setKind] = useState<ServerKind>(info.kind);
	const products = useProducts();
	const kinds = useProductKinds(product);
	const caps = useProductCaps(product);
	const canListPublicly = caps?.public_listing === true && kind === "central";
	const [rank, setRank] = useState<ServerRank | "">(info.rank ?? "");
	const [publicName, setPublicName] = useState<string>(info.public_name ?? "");
	// `is_monitored` carries the on/off toggle; `alert_when_down_for` is the
	// (always-positive) threshold to use when monitored. Stored separately
	// so muting doesn't lose the chosen threshold. UI works in minutes.
	const [isMonitored, setIsMonitored] = useState(info.is_monitored);
	// Off means "alert on everything else, just not this server going away".
	// Stored as the server-scoped silence on canopy's reachability check, the
	// same one the check itself offers, so the two surfaces are one state.
	// spec: CHK#operator-controls
	const alertsWhenUnreachableInitially = !reachabilitySilenced;
	const [alertWhenUnreachable, setAlertWhenUnreachable] = useState(
		alertsWhenUnreachableInitially,
	);
	const [alertWhenDownMinutes, setAlertWhenDownMinutes] = useState<string>(
		Math.max(1, Math.round(info.alert_when_down_for / 60)).toString(),
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
	const [mayManageDns, setMayManageDns] = useState(info.may_manage_dns);
	const [mayManageTls, setMayManageTls] = useState(info.may_manage_tls);

	const pending = action.pending || silence.pending || unsilence.pending;
	const error = action.error ?? silence.error ?? unsilence.error;

	const onSubmit = async (e: React.FormEvent) => {
		e.preventDefault();
		if (!groupId || !name.trim()) return; // name and group are required
		const data: Record<string, unknown> = {
			name: name.trim(),
			// Empty string clears the URL (server identified by its device only).
			host: host.trim(),
			product,
			kind,
			rank: rank === "" ? null : rank,
			// Sent whether or not the field is currently offered: a public name
			// already set survives the server losing eligibility, and takes effect
			// again if it regains it.
			// spec: APP#public-listing
			public_name: publicName.trim() === "" ? null : publicName.trim(),
			group_id: groupId,
			device_id: deviceId.trim() === "" ? null : deviceId.trim(),
			cloud: cloud === "" ? null : cloud === "true",
			geolocation:
				lat && lon
					? { lat: Number(lat), lon: Number(lon) }
					: null,
			is_monitored: isMonitored,
			alert_when_down_for: Math.max(
				60,
				Math.round(Number(alertWhenDownMinutes) * 60),
			),
			notes,
			tags,
			may_manage_dns: mayManageDns,
			may_manage_tls: mayManageTls,
		};
		try {
			await action.call({ server_id: info.id, data });
			if (alertWhenUnreachable !== alertsWhenUnreachableInitially) {
				const ref = {
					server_id: info.id,
					source: REACHABILITY_CHECK.source,
					ref: REACHABILITY_CHECK.ref,
				};
				await (alertWhenUnreachable
					? unsilence.call(ref)
					: silence.call(ref));
			}
			navigate(`/servers/${info.id}`);
		} catch {
			/* surfaced via the actions' errors */
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
					disabled={pending}
					required
				/>
				<TextField
					label="URL"
					value={host}
					onChange={(e) => setHost(e.target.value)}
					disabled={pending}
				/>
				<TextField
					select
					label="Product"
					value={product}
					onChange={(e) => {
						const next = e.target.value as Product;
						setProduct(next);
						// A role its new product doesn't define would leave the
						// server misclassified, so follow the product. The
						// endpoint applies the same rule if we don't.
						// spec: APP#product-and-kind
						const info = products.find((p) => p.product === next);
						if (info && !info.kinds.includes(kind)) {
							setKind(info.default_kind);
						}
					}}
					disabled={pending}
				>
					{products.map((p) => (
						<MenuItem key={p.product} value={p.product}>
							{PRODUCT_LABELS[p.product]}
						</MenuItem>
					))}
				</TextField>
				<TextField
					select
					label="Kind"
					value={kind}
					onChange={(e) => setKind(e.target.value as ServerKind)}
					disabled={pending || kinds.length < 2}
					helperText={
						kinds.length < 2
							? `${PRODUCT_LABELS[product]} servers have one role`
							: undefined
					}
				>
					{kinds.map((k) => (
						<MenuItem key={k} value={k}>
							{k}
						</MenuItem>
					))}
				</TextField>
				<TextField
					select
					label="Rank"
					value={rank}
					onChange={(e) => setRank(e.target.value as ServerRank | "")}
					disabled={pending}
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
					disabled={pending}
				/>

				<GroupControl
					currentGroupId={groupId}
					onChange={setGroupId}
					disabled={pending}
					required
				/>

				<Stack direction={{ xs: "column", md: "row" }} spacing={2}>
					<TextField
						select
						label="Location"
						value={cloud}
						onChange={(e) => setCloud(e.target.value as "" | "true" | "false")}
						disabled={pending}
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
						disabled={pending}
						sx={{ flex: 1 }}
					/>
					<TextField
						label="Longitude"
						value={lon}
						onChange={(e) => setLon(e.target.value)}
						disabled={pending}
						sx={{ flex: 1 }}
					/>
				</Stack>

				{canListPublicly && (
					<TextField
						label="Name in Tamanu Mobile app"
						value={publicName}
						onChange={(e) => setPublicName(e.target.value)}
						disabled={pending}
						helperText="Leave empty to hide this server from the public mobile-app list."
					/>
				)}

				<FormControlLabel
					control={
						<Checkbox
							checked={isMonitored}
							onChange={(e) => setIsMonitored(e.target.checked)}
							disabled={pending}
						/>
					}
					label="Monitor this server"
				/>
				<Typography variant="caption" color="text.secondary">
					When off, no check on this server alerts: its checks are still
					determined and shown, and its health and reachability are marked
					as unmonitored wherever they appear, but nothing triggers or joins
					an incident. Use this for test environments and ad-hoc demos that
					are expected to be down.
				</Typography>

				<FormControlLabel
					control={
						<Checkbox
							checked={alertWhenUnreachable}
							onChange={(e) => setAlertWhenUnreachable(e.target.checked)}
							disabled={pending}
						/>
					}
					label="Alert when this server is unreachable"
				/>
				<Typography variant="caption" color="text.secondary">
					When off, every other check alerts as normal and only the server
					going away is quiet. This is the same as silencing the
					reachability check on this server, and either place reflects the
					other.
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
						disabled={pending || !isMonitored || !alertWhenUnreachable}
						slotProps={{ htmlInput: { min: 1, step: 1 } }}
						sx={{ width: 140 }}
					/>
				</Stack>
				<Typography variant="caption" color="text.secondary">
					Raise this for flappy servers (so brief blips don't fire) or lower
					it for critical servers that should page promptly. The value is
					preserved while either switch above is off.
				</Typography>

				<NameManagementGrants
					groupId={groupId}
					mayManageDns={mayManageDns}
					mayManageTls={mayManageTls}
					setMayManageDns={setMayManageDns}
					setMayManageTls={setMayManageTls}
					disabled={action.pending}
				/>

				<TextField
					label="Notes"
					multiline
					minRows={3}
					value={notes}
					onChange={(e) => setNotes(e.target.value)}
					disabled={pending}
					helperText="Operator notes shown on the server's detail page. Plain text."
				/>

				<Stack spacing={1}>
					<Typography variant="subtitle1">Tags</Typography>
					<TagsEditor value={tags} onChange={setTags} disabled={pending} />
				</Stack>

				{error && <Alert severity="error">{error.message}</Alert>}

				<Stack direction="row" spacing={1}>
					<Button
						type="submit"
						variant="contained"
						disabled={pending || !groupId || !name.trim()}
					>
						{pending ? "Saving…" : "Save"}
					</Button>
					<Button
						type="button"
						variant="outlined"
						color="error"
						onClick={() => navigate(`/servers/${info.id}`)}
						disabled={pending}
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

/// The two name-management grants, shown only when they could mean something.
///
/// A grant is exercised over names beneath a domain the server's *group*
/// controls, so it is worth nothing on its own. Where the Canopy instance has no zones
/// and no group anywhere controls a domain, the feature is not in use at all and
/// these controls stay out of the way entirely — a checkbox that cannot affect
/// anything is worse than no checkbox. Where the feature is in use but this
/// group controls no domain, they show disabled with the reason, because that is
/// a gap an operator can close by claiming a domain.
///
/// Keyed on the *selected* group rather than the saved one, so moving the server
/// into a group that controls a domain makes the grants available before saving.
// spec: DOM#permission-for-a-server-to-manage-its-own-names
function NameManagementGrants({
	groupId,
	mayManageDns,
	mayManageTls,
	setMayManageDns,
	setMayManageTls,
	disabled,
}: {
	groupId: string | null;
	mayManageDns: boolean;
	mayManageTls: boolean;
	setMayManageDns: (v: boolean) => void;
	setMayManageTls: (v: boolean) => void;
	disabled: boolean;
}) {
	const availability = useApi(
		"domains",
		"grant_availability",
		{ server_group_id: groupId },
		[groupId],
	);

	// Wait for the answer rather than guessing: rendering enabled controls and
	// then disabling them reads as the form fighting the operator.
	if (availability.status !== "ok") return null;
	const { state, group_domains } = availability.data;

	const held = mayManageDns || mayManageTls;

	// Not in use in this Canopy instance — unless this server somehow holds a grant
	// already, in which case hiding the control would strand it with no way to
	// withdraw it.
	if (state === "unconfigured" && !held) return null;

	const unavailable = state !== "available";

	return (
		<Stack spacing={1}>
			<Typography variant="subtitle1">Name management</Typography>
			<FormControlLabel
				control={
					<Checkbox
						checked={mayManageDns}
						onChange={(e) => setMayManageDns(e.target.checked)}
						disabled={disabled || (unavailable && !mayManageDns)}
					/>
				}
				label="May manage its own DNS records"
			/>
			<FormControlLabel
				control={
					<Checkbox
						checked={mayManageTls}
						onChange={(e) => setMayManageTls(e.target.checked)}
						disabled={disabled || (unavailable && !mayManageTls)}
					/>
				}
				label="May obtain its own TLS certificates"
			/>
			{state === "available" ? (
				<Typography variant="caption" color="text.secondary">
					Both apply only to names under {group_domains.join(", ")}, the
					domain{group_domains.length === 1 ? "" : "s"} this server's group
					controls, and are off until granted: a server without the grant it
					needs is refused. Revoking stops further changes and leaves records
					and certificates already in place.
				</Typography>
			) : state === "no_group_domain" ? (
				<Alert severity="info">
					{groupId
						? "This server's group controls no domain, so neither grant would authorise it over any name. Claim a domain on the group's page first."
						: "This server has no group. A domain is controlled by a group, so a server outside one can hold no useful grant."}
				</Alert>
			) : (
				<Alert severity="warning">
					Canopy has no managed DNS zones configured and no group controls a
					domain, so name management is not in use here — but this server still
					holds a grant. Clear it, or have the infrastructure provide a zone.
				</Alert>
			)}
		</Stack>
	);
}
