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
import PreviewIcon from "@mui/icons-material/Preview";
import { Fragment } from "react";
import { useApi, type ApiState } from "../api";
import TimeAgo from "./TimeAgo";
import VersionIndicator from "./VersionIndicator";
import type { StatusSnapshotData } from "../types";

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
			<ChecksBlock health={snap.health} />
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
			{snap.timezone && <Field label="Timezone" value={snap.timezone} />}
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

function ChecksBlock({ health }: { health: StatusSnapshotData["health"] }) {
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
						{entry.healthy ? (
							<CheckCircleIcon fontSize="small" color="success" />
						) : (
							<CancelIcon fontSize="small" color="error" />
						)}
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
												sx={{ m: 0, fontFamily: "monospace" }}
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
