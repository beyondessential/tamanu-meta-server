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
import { useEffect, useRef, useState } from "react";
import { useApi, useApiAction } from "../api";
import { useReloadInterval } from "../hooks/useReloadInterval";
import TimeAgo from "./TimeAgo";
import type { EnrollmentTicket } from "../types";

/// Setup / enrollment instructions for a not-yet-registered server. Mints an
/// encrypted enrollment ticket plus its 4-word passphrase, shows the ticket
/// for the operator to paste into `bestool canopy register` and the passphrase
/// to share out-of-band, the token expiry, a reissue button, and a live
/// "waiting for check-in" → "registered" indicator polled from
/// `servers.enrollment_status`.
export default function ServerSetupInstructions({
	serverId,
	onRegistered,
	reEnroll = false,
}: {
	serverId: string;
	/// Fired once when the server first reports `registered_at`. The parent
	/// can use this to refresh the surrounding page (e.g. flip the detail
	/// view out of its "not registered" banner).
	onRegistered?: () => void;
	/// Re-enrollment of an already-registered server: "registered" is judged by
	/// `registered_at` *changing* from its value at mount (a new device
	/// completing the handshake), not merely being set.
	reEnroll?: boolean;
}) {
	const mint = useApiAction("servers", "mint_enrollment");
	const [ticket, setTicket] = useState<EnrollmentTicket | null>(null);
	const [copied, setCopied] = useState(false);
	const [copiedPassphrase, setCopiedPassphrase] = useState(false);

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

	// In re-enroll mode the server is already registered, so "done" means the
	// `registered_at` timestamp has *changed* since we opened (a new device
	// completed). Capture the value at first status load as the baseline.
	const baselineRegisteredAt = useRef<string | null | undefined>(undefined);
	useEffect(() => {
		if (status.status === "ok" && baselineRegisteredAt.current === undefined) {
			baselineRegisteredAt.current = status.data.registered_at ?? null;
		}
	}, [status]);
	const registeredView = reEnroll
		? registeredAt != null &&
			baselineRegisteredAt.current !== undefined &&
			registeredAt !== baselineRegisteredAt.current
		: registeredAt != null;

	// Mint a fresh ticket on mount / when the server changes.
	useEffect(() => {
		let cancelled = false;
		setTicket(null);
		mint
			.call({ server_id: serverId })
			.then((t) => {
				if (!cancelled) setTicket(t);
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
		if (registeredView && !notified) {
			setNotified(true);
			onRegistered?.();
		}
	}, [registeredView, notified, onRegistered]);

	const reissue = () => {
		setTicket(null);
		mint
			.call({ server_id: serverId })
			.then((t) => setTicket(t))
			.catch(() => {
				/* surfaced via mint.error */
			});
	};

	// The ticket is encrypted, so it's safe to put on the command line — the
	// whole thing is one copy-paste and bestool only prompts for the passphrase.
	const command = ticket ? `bestool canopy register ${ticket.ticket}` : "";

	const onCopy = async () => {
		if (!ticket) return;
		try {
			await navigator.clipboard.writeText(command);
			setCopied(true);
			window.setTimeout(() => setCopied(false), 2000);
		} catch {
			/* clipboard may be unavailable; ignore */
		}
	};

	const onCopyPassphrase = async () => {
		if (!ticket) return;
		try {
			await navigator.clipboard.writeText(ticket.passphrase);
			setCopiedPassphrase(true);
			window.setTimeout(() => setCopiedPassphrase(false), 2000);
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
						{reEnroll ? "Re-enroll a device" : "Set up this server"}
					</Typography>
					<RegistrationState
						registered={registeredView}
						tokenExpiresAt={
							status.status === "ok"
								? status.data.token_expires_at
								: null
						}
					/>
				</Stack>

				<Stack
					direction="row"
					spacing={1}
					sx={{ alignItems: "center" }}
				>
					<Typography
						variant="body2"
						color="text.secondary"
						sx={{ flex: 1 }}
					>
						Run this on the {reEnroll ? "replacement " : ""}server; it
						will prompt for the passphrase shown below.
					</Typography>
					{ticket && (
						<>
							<Tooltip title={copied ? "Copied" : "Copy command"}>
								<IconButton
									size="small"
									onClick={onCopy}
									aria-label="Copy register command"
								>
									<ContentCopyIcon fontSize="small" />
								</IconButton>
							</Tooltip>
							<Tooltip title="Generates a new ticket and passphrase; the current ones immediately stop working.">
								<Button
									size="small"
									startIcon={<RefreshIcon />}
									onClick={reissue}
									disabled={mint.pending}
								>
									{mint.pending ? "Reissuing…" : "Reissue"}
								</Button>
							</Tooltip>
						</>
					)}
				</Stack>

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
					{mint.pending && !ticket
						? "Minting enrollment ticket…"
						: (command || "—")}
				</Box>

				{ticket && (
					<Box>
						<Typography variant="subtitle2" gutterBottom>
							Passphrase
						</Typography>
						<Stack
							direction="row"
							spacing={1}
							sx={{ alignItems: "center" }}
						>
							<Box
								component="code"
								sx={{
									flex: 1,
									p: 1.5,
									borderRadius: 1,
									bgcolor: "action.selected",
									fontFamily: "monospace",
									fontSize: "1.1em",
									fontWeight: 600,
									letterSpacing: "0.02em",
									userSelect: "all",
									wordBreak: "break-all",
								}}
							>
								{ticket.passphrase}
							</Box>
							<Tooltip
								title={copiedPassphrase ? "Copied" : "Copy passphrase"}
							>
								<IconButton
									size="small"
									onClick={onCopyPassphrase}
									aria-label="Copy passphrase"
								>
									<ContentCopyIcon fontSize="small" />
								</IconButton>
							</Tooltip>
						</Stack>
						<Typography
							variant="caption"
							color="text.secondary"
							sx={{ display: "block", mt: 1 }}
						>
							Share the passphrase over a separate channel (e.g. a
							call).
						</Typography>
					</Box>
				)}

				{mint.error && (
					<Alert severity="error">{mint.error.message}</Alert>
				)}
			</Stack>
		</Paper>
	);
}

function RegistrationState({
	registered,
	tokenExpiresAt,
}: {
	registered: boolean;
	tokenExpiresAt: string | null;
}) {
	if (registered) {
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
