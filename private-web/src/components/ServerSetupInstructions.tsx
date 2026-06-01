import {
	Alert,
	Box,
	Button,
	Chip,
	CircularProgress,
	IconButton,
	Paper,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import RefreshIcon from "@mui/icons-material/Refresh";
import VisibilityIcon from "@mui/icons-material/Visibility";
import VisibilityOffIcon from "@mui/icons-material/VisibilityOff";
import { useEffect, useState } from "react";
import { useApi, useApiAction } from "../api";
import { useReloadInterval } from "../hooks/useReloadInterval";
import TimeAgo from "./TimeAgo";
import type { EnrollmentBlob } from "../types";

/// Setup / enrollment instructions for a not-yet-registered server. Mints an
/// enrollment blob, shows the `bestool canopy register` command (with the
/// blob masked until revealed), the token expiry, a reissue button, and a
/// live "waiting for check-in" → "registered" indicator polled from
/// `servers.enrollment_status`.
export default function ServerSetupInstructions({
	serverId,
	onRegistered,
}: {
	serverId: string;
	/// Fired once when the server first reports `registered_at`. The parent
	/// can use this to refresh the surrounding page (e.g. flip the detail
	/// view out of its "not registered" banner).
	onRegistered?: () => void;
}) {
	const mint = useApiAction("servers", "mint_enrollment");
	const [blob, setBlob] = useState<EnrollmentBlob | null>(null);
	const [revealed, setRevealed] = useState(false);
	const [copied, setCopied] = useState(false);

	// Poll enrollment status while we're showing instructions.
	const tick = useReloadInterval(5000);
	const status = useApi(
		"servers",
		"enrollment_status",
		{ server_id: serverId },
		[serverId, tick],
	);
	const registeredAt =
		status.status === "ok" ? status.data.registered_at : null;

	// Mint a fresh blob on mount / when the server changes.
	useEffect(() => {
		let cancelled = false;
		setBlob(null);
		setRevealed(false);
		mint
			.call({ server_id: serverId })
			.then((b) => {
				if (!cancelled) setBlob(b);
			})
			.catch(() => {
				/* surfaced via mint.error */
			});
		return () => {
			cancelled = true;
		};
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [serverId]);

	// Notify the parent once when registration first completes.
	const [notified, setNotified] = useState(false);
	useEffect(() => {
		if (registeredAt && !notified) {
			setNotified(true);
			onRegistered?.();
		}
	}, [registeredAt, notified, onRegistered]);

	const reissue = () => {
		setRevealed(false);
		setBlob(null);
		mint
			.call({ server_id: serverId })
			.then((b) => setBlob(b))
			.catch(() => {
				/* surfaced via mint.error */
			});
	};

	const command = blob
		? `bestool canopy register${revealed ? `\n${blob.blob}` : ""}`
		: "";

	const onCopy = async () => {
		if (!blob) return;
		try {
			await navigator.clipboard.writeText(blob.blob);
			setCopied(true);
			window.setTimeout(() => setCopied(false), 2000);
		} catch {
			/* clipboard may be unavailable; ignore */
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack spacing={2}>
				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center", justifyContent: "space-between" }}
				>
					<Typography variant="h6" component="h2">
						Set up this server
					</Typography>
					<RegistrationState
						registeredAt={registeredAt}
						tokenExpiresAt={
							status.status === "ok"
								? status.data.token_expires_at
								: null
						}
					/>
				</Stack>

				<Typography variant="body2" color="text.secondary">
					Install bestool on the server, then run the command below. Paste
					the enrollment blob into its standard input — for example:
				</Typography>

				<Box
					component="pre"
					sx={{
						m: 0,
						p: 1.5,
						borderRadius: 1,
						bgcolor: "action.hover",
						overflow: "auto",
						fontSize: "0.85em",
						fontFamily: "monospace",
						whiteSpace: "pre-wrap",
						wordBreak: "break-all",
					}}
				>
					{mint.pending && !blob
						? "Minting enrollment token…"
						: command || "—"}
				</Box>

				{blob && (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						<Tooltip title={revealed ? "Hide blob" : "Reveal blob"}>
							<IconButton
								size="small"
								onClick={() => setRevealed((v) => !v)}
								aria-label={
									revealed ? "Hide enrollment blob" : "Reveal enrollment blob"
								}
							>
								{revealed ? (
									<VisibilityOffIcon fontSize="small" />
								) : (
									<VisibilityIcon fontSize="small" />
								)}
							</IconButton>
						</Tooltip>
						<Tooltip title={copied ? "Copied" : "Copy blob"}>
							<IconButton
								size="small"
								onClick={onCopy}
								aria-label="Copy enrollment blob"
							>
								<ContentCopyIcon fontSize="small" />
							</IconButton>
						</Tooltip>
						<Button
							size="small"
							startIcon={<RefreshIcon />}
							onClick={reissue}
							disabled={mint.pending}
						>
							{mint.pending ? "Reissuing…" : "Reissue"}
						</Button>
						<Box sx={{ flex: 1 }} />
						<Typography variant="caption" color="text.secondary">
							The blob is sensitive — anyone holding it can enroll as this
							server.
						</Typography>
					</Stack>
				)}

				{mint.error && (
					<Alert severity="error">{mint.error.message}</Alert>
				)}
			</Stack>
		</Paper>
	);
}

function RegistrationState({
	registeredAt,
	tokenExpiresAt,
}: {
	registeredAt: string | null;
	tokenExpiresAt: string | null;
}) {
	if (registeredAt) {
		return (
			<Chip
				size="small"
				color="success"
				icon={<CheckCircleIcon />}
				label="registered ✓"
			/>
		);
	}
	return (
		<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
			<CircularProgress size={14} />
			<Typography variant="body2" color="text.secondary">
				waiting for this server to check in…
				{tokenExpiresAt && (
					<>
						{" "}
						(token expires <TimeAgo timestamp={tokenExpiresAt} />)
					</>
				)}
			</Typography>
		</Stack>
	);
}
