import {
	Alert,
	Box,
	Chip,
	IconButton,
	LinearProgress,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import CancelIcon from "@mui/icons-material/Cancel";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import CloseIcon from "@mui/icons-material/Close";
import CircleIcon from "@mui/icons-material/Circle";
import ErrorIcon from "@mui/icons-material/Error";
import InfoIcon from "@mui/icons-material/Info";
import PreviewIcon from "@mui/icons-material/Preview";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { Fragment } from "react";
import { useApi, type ApiState } from "../api";
import TimeAgo from "./TimeAgo";
import TimezoneTooltip from "./TimezoneTooltip";
import VersionIndicator from "./VersionIndicator";
import {
	SEVERITY_INTENT,
	type Severity,
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
						<Chip
							size="small"
							color={result.data.healthy ? "success" : "error"}
							icon={
								result.data.healthy ? <CheckCircleIcon /> : <CancelIcon />
							}
							label={result.data.healthy ? "Healthy" : "Unhealthy"}
						/>
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
			<ChecksBlock health={snap.health} severities={snap.check_severities} />
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
	severities,
}: {
	health: StatusSnapshotData["health"];
	severities: StatusSnapshotData["check_severities"];
}) {
	const entries = parseChecks(health);
	if (entries.length === 0) return null;
	return (
		<Box>
			<Typography variant="overline" color="text.secondary">
				Health checks ({entries.length})
			</Typography>
			<Stack spacing={1} sx={{ mt: 0.5 }}>
				{entries.map((entry) => (
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
							bgcolor: entry.healthy ? undefined : "action.hover",
						}}
					>
						<CheckIcon
							healthy={entry.healthy}
							severity={severities[entry.check] ?? null}
						/>
						<Box sx={{ flex: 1, minWidth: 0 }}>
							<Typography variant="body2" sx={{ fontFamily: "monospace" }}>
								{entry.check}
							</Typography>
							{entry.extras.length > 0 && (
								<Box
									component="dl"
									sx={{
										m: 0,
										mt: 0.5,
										display: "grid",
										gridTemplateColumns: "max-content 1fr",
										columnGap: 1.5,
										rowGap: 0.25,
										fontSize: "0.8em",
									}}
								>
									{entry.extras.map(([k, v]) => (
										<Fragment key={k}>
											<Box component="dt" sx={{ color: "text.secondary" }}>
												{k}
											</Box>
											<Box
												component="dd"
												sx={{
													m: 0,
													fontFamily: "monospace",
													minWidth: 0,
													overflowWrap: "anywhere",
												}}
											>
												{renderValue(v)}
											</Box>
										</Fragment>
									))}
								</Box>
							)}
						</Box>
					</Stack>
				))}
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
	healthy: boolean;
	extras: Array<[string, unknown]>;
};

function parseChecks(health: StatusSnapshotData["health"]): ParsedCheck[] {
	if (!Array.isArray(health)) return [];
	const parsed: ParsedCheck[] = [];
	for (const raw of health as unknown[]) {
		if (typeof raw !== "object" || raw === null) continue;
		const obj = raw as Record<string, unknown>;
		const check = obj.check;
		const healthy = obj.healthy;
		if (typeof check !== "string" || typeof healthy !== "boolean") continue;
		const extras: Array<[string, unknown]> = Object.entries(obj).filter(
			([k]) => k !== "check" && k !== "healthy",
		);
		parsed.push({ check, healthy, extras });
	}
	parsed.sort((a, b) => {
		if (a.healthy !== b.healthy) return a.healthy ? 1 : -1;
		return a.check.localeCompare(b.check);
	});
	return parsed;
}

/// Per-check status indicator. Healthy checks always render as a green
/// tick. Unhealthy checks render the icon for the rules engine's
/// computed severity (debug → grey dot, info → blue i, warning →
/// yellow triangle, error → red ⊘, critical → red filled exclamation).
/// Falls back to the warning icon when the severity is absent — the
/// catalog hasn't been touched for this check yet, so we surface it
/// at the default level rather than miscolouring it.
function CheckIcon({
	healthy,
	severity,
}: {
	healthy: boolean;
	severity: Severity | null;
}) {
	if (healthy) {
		return (
			<Tooltip title="Passing" arrow>
				<CheckCircleIcon fontSize="small" color="success" />
			</Tooltip>
		);
	}
	const sev: Severity = severity ?? "warning";
	const tooltip = `${sev} — ${SEVERITY_INTENT[sev]}`;
	switch (sev) {
		case "critical":
			return (
				<Tooltip title={tooltip} arrow>
					<ErrorIcon fontSize="small" color="error" />
				</Tooltip>
			);
		case "error":
			return (
				<Tooltip title={tooltip} arrow>
					<CancelIcon fontSize="small" color="error" />
				</Tooltip>
			);
		case "warning":
			return (
				<Tooltip title={tooltip} arrow>
					<WarningAmberIcon fontSize="small" color="warning" />
				</Tooltip>
			);
		case "info":
			return (
				<Tooltip title={tooltip} arrow>
					<InfoIcon fontSize="small" color="info" />
				</Tooltip>
			);
		case "debug":
			return (
				<Tooltip title={tooltip} arrow>
					<CircleIcon fontSize="small" color="disabled" sx={{ fontSize: 12 }} />
				</Tooltip>
			);
	}
}

function renderValue(v: unknown): string {
	if (typeof v === "string") return v;
	if (v === null) return "null";
	return JSON.stringify(v);
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
