import {
	Alert,
	Box,
	IconButton,
	LinearProgress,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import BuildCircleIcon from "@mui/icons-material/BuildCircle";
import CancelIcon from "@mui/icons-material/Cancel";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import CloseIcon from "@mui/icons-material/Close";
import CircleIcon from "@mui/icons-material/Circle";
import InfoIcon from "@mui/icons-material/Info";
import NotificationsOffIcon from "@mui/icons-material/NotificationsOff";
import PreviewIcon from "@mui/icons-material/Preview";
import RemoveCircleOutlinedIcon from "@mui/icons-material/RemoveCircleOutlined";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { useApi, type ApiState } from "../api";
import CheckExtrasList, { checkEntryExtras } from "./CheckExtras";
import ExternalUsersDetails, {
	parseExternalUserSessions,
} from "./ExternalUsersDetails";
import HealthChip from "./HealthChip";
import TimeAgo from "./TimeAgo";
import TimezoneTooltip from "./TimezoneTooltip";
import VersionIndicator from "./VersionIndicator";
import {
	CHECK_RESULT_INTENT,
	CHECK_RESULT_ORDER,
	checkResultOf,
	type CheckResult,
	type StatusSnapshotData,
} from "../types";

/** Inline panel rendering the server's status at a given point in time.
 * Hits `/api/statuses/snapshot` lazily on mount; each (serverId, at)
 * gets its own component instance via the caller's keying so there is no
 * stale-data flash when the user toggles between snapshots. Two or more
 * panels can be open at once for side-by-side comparison. */
export default function StatusSnapshotPanel({
	serverId,
	at,
	onClose,
}: {
	serverId: string;
	/** Timestamp to look up "as of". When null, the endpoint returns
	 * the latest status. */
	at: string | null;
	onClose: () => void;
}) {
	const result = useApi(
		"statuses",
		"snapshot",
		{ server_id: serverId, at },
		[serverId, at],
	);
	return (
		<Box
			sx={{
				mt: 1,
				p: 1.5,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
			}}
		>
			<Stack
				direction="row"
				spacing={1.5}
				sx={{ alignItems: "center", flexWrap: "wrap", mb: 1 }}
			>
				<Typography variant="overline" color="text.secondary">
					Status snapshot
				</Typography>
				{result.status === "ok" && result.data && (
					<>
						<HealthChip health={result.data.health_state} />
						<Typography variant="body2" color="text.secondary">
							<TimeAgo timestamp={result.data.created_at} />
						</Typography>
					</>
				)}
				<Box sx={{ ml: "auto" }}>
					<IconButton aria-label="Close" size="small" onClick={onClose}>
						<CloseIcon fontSize="small" />
					</IconButton>
				</Box>
			</Stack>
			<PanelBody result={result} />
		</Box>
	);
}

function PanelBody({
	result,
}: {
	result: ApiState<StatusSnapshotData | null>;
}) {
	if (result.status === "loading" || result.status === "idle") {
		return <LinearProgress />;
	}
	if (result.status === "error") {
		return <Alert severity="error">{result.error.message}</Alert>;
	}
	if (result.data === null) {
		return (
			<Alert severity="info">
				No status snapshot is available for this point in time — the server
				hadn't reported yet.
			</Alert>
		);
	}
	const snap = result.data;
	return (
		<Stack spacing={2}>
			<CuratedFields snap={snap} />
			<ChecksBlock
				health={snap.health}
				results={snap.check_results}
				operators={snap.operators}
				silencedChecks={snap.silenced_checks}
			/>
			<ExtrasBlock extra={snap.extra} />
		</Stack>
	);
}

function CuratedFields({ snap }: { snap: StatusSnapshotData }) {
	return (
		<Stack direction="row" spacing={3} sx={{ flexWrap: "wrap" }} useFlexGap>
			<Field label="Tamanu">
				<VersionIndicator
					version={snap.version}
					distance={snap.version_distance}
				/>
			</Field>
			{snap.platform && <Field label="Platform" value={snap.platform} />}
			{snap.timezone && (
				<Field label="Timezone">
					<Typography variant="body2">
						<TimezoneTooltip tz={snap.timezone} />
					</Typography>
				</Field>
			)}
			{snap.postgres && <Field label="PostgreSQL" value={snap.postgres} mono />}
			{snap.nodejs && <Field label="Node.js" value={snap.nodejs} mono />}
			{snap.min_chrome_version != null && (
				<Field
					label="Chrome"
					value={`${snap.min_chrome_version} or later`}
					mono
				/>
			)}
		</Stack>
	);
}

function Field({
	label,
	value,
	mono = false,
	children,
}: {
	label: string;
	value?: string;
	mono?: boolean;
	children?: React.ReactNode;
}) {
	return (
		<Stack spacing={0.25}>
			<Typography variant="caption" color="text.secondary">
				{label}
			</Typography>
			{children ?? (
				<Typography
					variant="body2"
					sx={mono ? { fontFamily: "monospace" } : undefined}
				>
					{value}
				</Typography>
			)}
		</Stack>
	);
}

function ChecksBlock({
	health,
	results,
	operators,
	silencedChecks,
}: {
	health: StatusSnapshotData["health"];
	results: StatusSnapshotData["check_results"];
	operators: StatusSnapshotData["operators"];
	silencedChecks: StatusSnapshotData["silenced_checks"];
}) {
	// Silenced checks render skip-style and sort with the skipped tail —
	// the backend excludes them from `health_state` the same way.
	const entries = parseChecks(health, new Set(silencedChecks));
	if (entries.length === 0) return null;
	return (
		<Box>
			<Typography variant="overline" color="text.secondary">
				Health checks ({entries.length})
			</Typography>
			<Stack spacing={1} sx={{ mt: 0.5 }}>
				{entries.map((entry) => {
					// Same special-case as the ServerDetail checks table:
					// formatted session rows for `external_users`, generic
					// dl fallback when the shape is unexpected. No "right
					// now" claim here — a snapshot is "as of" its push.
					const sessions =
						entry.check === "external_users"
							? parseExternalUserSessions(entry.extras)
							: null;
					const extras =
						sessions === null
							? entry.extras
							: entry.extras.filter(
									([k]) => k !== "users" && k !== "count",
								);
					return (
						<Stack
							key={entry.check}
							direction="row"
							spacing={1.5}
							sx={{
								p: 1,
								border: 1,
								borderColor: "divider",
								borderRadius: 1,
								alignItems: "flex-start",
								bgcolor:
									entry.result === "passed" ||
									entry.result === "skipped" ||
									entry.silenced
										? undefined
										: "action.hover",
							}}
						>
							<CheckIcon
								result={entry.result}
								effective={
									(results[entry.check] as CheckResult | undefined) ?? null
								}
								silenced={entry.silenced}
							/>
							<Box sx={{ flex: 1, minWidth: 0 }}>
								<Typography variant="body2" sx={{ fontFamily: "monospace" }}>
									{entry.check}
								</Typography>
								{sessions !== null && (
									<ExternalUsersDetails
										sessions={sessions}
										operators={operators}
									/>
								)}
								<CheckExtrasList extras={extras} />
							</Box>
						</Stack>
					);
				})}
			</Stack>
		</Box>
	);
}

function ExtrasBlock({ extra }: { extra: StatusSnapshotData["extra"] }) {
	const obj = (extra ?? {}) as Record<string, unknown>;
	if (Object.keys(obj).length === 0) return null;
	return (
		<Box>
			<details>
				<summary>Raw payload</summary>
				<Box
					component="pre"
					sx={{
						mt: 1,
						p: 1.5,
						borderRadius: 1,
						bgcolor: "action.hover",
						overflow: "auto",
						fontSize: "0.85em",
					}}
				>
					{JSON.stringify(extra, null, 2)}
				</Box>
			</details>
		</Box>
	);
}

type ParsedCheck = {
	check: string;
	result: CheckResult;
	/** Whether the check is silenced (server or group scope), per the
	 * snapshot's `silenced_checks`: presented skip-style and excluded
	 * from the health rollup. */
	silenced: boolean;
	extras: Array<[string, unknown]>;
};

function parseChecks(
	health: StatusSnapshotData["health"],
	silencedChecks: Set<string>,
): ParsedCheck[] {
	if (!Array.isArray(health)) return [];
	const parsed: ParsedCheck[] = [];
	for (const raw of health as unknown[]) {
		if (typeof raw !== "object" || raw === null) continue;
		const obj = raw as Record<string, unknown>;
		const check = obj.check;
		const result = checkResultOf(obj);
		if (typeof check !== "string" || result === null) continue;
		parsed.push({
			check,
			result,
			silenced: silencedChecks.has(check),
			extras: checkEntryExtras(obj),
		});
	}
	const sortResult = (e: ParsedCheck): CheckResult =>
		e.silenced ? "skipped" : e.result;
	parsed.sort((a, b) => {
		if (sortResult(a) !== sortResult(b)) {
			return (
				CHECK_RESULT_ORDER.indexOf(sortResult(a)) -
				CHECK_RESULT_ORDER.indexOf(sortResult(b))
			);
		}
		return a.check.localeCompare(b.check);
	});
	return parsed;
}

/// Per-check status indicator. Passed checks render as a green tick;
/// broken checks (the check itself errored, not the system) as an
/// orange wrench; skipped checks (precondition not met) as a grey
/// dash. Warning/failed checks render the icon for the rules engine's
/// computed severity (debug → grey dot, info → blue i, warning →
/// yellow triangle, error → red ⊘, critical → red filled exclamation).
/// Falls back to the warning icon when the severity is absent — the
/// catalog hasn't been touched for this check yet, so we surface it
/// at the default level rather than miscolouring it. Silenced checks
/// get the same neutral grey treatment as skipped ones, whatever they
/// reported — they don't count toward the server's health.
function CheckIcon({
	result,
	effective,
	silenced = false,
}: {
	result: CheckResult;
	effective: CheckResult | null;
	silenced?: boolean;
}) {
	if (silenced) {
		return (
			<Tooltip
				title={`Silenced — reported ${result}, not counted toward server health`}
				arrow
			>
				<NotificationsOffIcon fontSize="small" color="disabled" />
			</Tooltip>
		);
	}
	switch (result) {
		case "passed":
			return (
				<Tooltip title="Passing" arrow>
					<CheckCircleIcon fontSize="small" color="success" />
				</Tooltip>
			);
		case "broken":
			return (
				<Tooltip
					title="Broken — the check itself is failing, not the system under test"
					arrow
				>
					<BuildCircleIcon fontSize="small" color="warning" />
				</Tooltip>
			);
		case "skipped":
			return (
				<Tooltip title="Skipped — a precondition was not met" arrow>
					<RemoveCircleOutlinedIcon fontSize="small" color="disabled" />
				</Tooltip>
			);
		case "warning":
		case "failed":
			break;
	}
	// A degraded observation renders by what policy grades it to.
	const eff: CheckResult = effective ?? "warning";
	const tooltip =
		result === eff
			? `${result} — ${CHECK_RESULT_INTENT[eff]}`
			: `${result}, graded ${eff} — ${CHECK_RESULT_INTENT[eff]}`;
	switch (eff) {
		case "failed":
			return (
				<Tooltip title={tooltip} arrow>
					<CancelIcon fontSize="small" color="error" />
				</Tooltip>
			);
		case "warning":
		case "broken":
			return (
				<Tooltip title={tooltip} arrow>
					<WarningAmberIcon fontSize="small" color="warning" />
				</Tooltip>
			);
		case "passed":
			return (
				<Tooltip title={tooltip} arrow>
					<InfoIcon fontSize="small" color="info" />
				</Tooltip>
			);
		case "skipped":
			return (
				<Tooltip title={tooltip} arrow>
					<CircleIcon fontSize="small" color="disabled" sx={{ fontSize: 12 }} />
				</Tooltip>
			);
	}
}

/** Toggle button that opens an inline snapshot panel. Caller owns the
 * `open` state and decides where to render the panel — the button just
 * reflects open/closed visually. */
export function StatusSnapshotButton({
	open,
	onClick,
	tooltip = "View status snapshot",
}: {
	open: boolean;
	onClick: () => void;
	tooltip?: string;
}) {
	return (
		<Tooltip title={open ? "Close status snapshot" : tooltip}>
			<IconButton
				aria-label={tooltip}
				size="small"
				color={open ? "primary" : "default"}
				onClick={(e) => {
					e.stopPropagation();
					onClick();
				}}
			>
				<PreviewIcon fontSize="small" />
			</IconButton>
		</Tooltip>
	);
}
