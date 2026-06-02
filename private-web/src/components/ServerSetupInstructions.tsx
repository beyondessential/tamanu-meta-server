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
import { useEffect, useState } from "react";
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
}: {
	serverId: string;
	/// Fired once when the server first reports `registered_at`. The parent
	/// can use this to refresh the surrounding page (e.g. flip the detail
	/// view out of its "not registered" banner).
	onRegistered?: () => void;
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
		if (registeredAt && !notified) {
			setNotified(true);
			onRegistered?.();
		}
	}, [registeredAt, notified, onRegistered]);

	const reissue = () => {
		setTicket(null);
		mint
			.call({ server_id: serverId })
			.then((t) => setTicket(t))
			.catch(() => {
				/* surfaced via mint.error */
			});
	};

	const onCopy = async () => {
		if (!ticket) return;
		try {
			await navigator.clipboard.writeText(ticket.ticket);
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
					Run{" "}
					<Box
						component="code"
						sx={{
							fontFamily: "monospace",
							bgcolor: "action.hover",
							px: 0.5,
							borderRadius: 0.5,
						}}
					>
						bestool canopy register
					</Box>
					, paste the enrollment ticket below when prompted, and it
					will then ask for the passphrase shown further down.
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
					{mint.pending && !ticket
						? "Minting enrollment ticket…"
						: (ticket?.ticket ?? "—")}
				</Box>

				{ticket && (
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						<Tooltip title={copied ? "Copied" : "Copy ticket"}>
							<IconButton
								size="small"
								onClick={onCopy}
								aria-label="Copy enrollment ticket"
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
							The ticket is encrypted; it is useless without the
							passphrase.
						</Typography>
					</Stack>
				)}

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
							call). The ticket is useless without it.
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
