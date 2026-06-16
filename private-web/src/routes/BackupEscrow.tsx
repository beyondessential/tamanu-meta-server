import {
	Alert,
	AlertTitle,
	Box,
	Button,
	Checkbox,
	FormControlLabel,
	IconButton,
	Paper,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import VisibilityIcon from "@mui/icons-material/Visibility";
import { useState } from "react";
import { useApiAction } from "../api";
import { useIsAdmin } from "../hooks/useIsAdmin";
import type { BackupConfigView, RevealEscrowResponse } from "../types";

/// Reveal-once passphrase + Bitwarden acknowledgment. Rendered only while the
/// group's config is `escrow_pending` and `mode === "from_birth"`. The reveal
/// is deliberately re-callable until the operator acks (they may reload before
/// saving); once acked the row flips to `ready` and reveal 409s.
export default function BackupEscrow({
	config,
	onAcked,
}: {
	config: BackupConfigView;
	onAcked: () => void;
}) {
	const isAdmin = useIsAdmin() === true;
	const reveal = useApiAction<"backups", "reveal_escrow", RevealEscrowResponse>(
		"backups",
		"reveal_escrow",
	);
	const ack = useApiAction("backups", "ack_escrow");
	const [revealed, setRevealed] = useState<RevealEscrowResponse | null>(null);
	const [saved, setSaved] = useState(false);

	if (config.status !== "escrow_pending" || config.mode !== "from_birth") {
		return null;
	}

	const onReveal = async () => {
		try {
			const r = await reveal.call({ server_group_id: config.server_group_id });
			setRevealed(r);
		} catch {
			/* surfaced via reveal.error */
		}
	};

	const onAck = async () => {
		try {
			await ack.call({ server_group_id: config.server_group_id });
			onAcked();
		} catch {
			/* surfaced via ack.error */
		}
	};

	return (
		<Paper
			variant="outlined"
			sx={{ p: 2, borderColor: "warning.main", borderWidth: 2 }}
		>
			<Stack spacing={2}>
				<Typography variant="h6" component="h2">
					Escrow the repository passphrase
				</Typography>
				<Alert severity="warning">
					<AlertTitle>One-time reveal</AlertTitle>
					This repository's passphrase can be shown here once for you to copy.
					Save it to Bitwarden, then acknowledge below to activate backups.
				</Alert>

				{!isAdmin && (
					<Alert severity="info">
						Only admins can reveal and acknowledge the passphrase.
					</Alert>
				)}

				{isAdmin && !revealed && (
					<Box>
						<Button
							variant="contained"
							color="warning"
							startIcon={<VisibilityIcon />}
							onClick={onReveal}
							disabled={reveal.pending}
						>
							{reveal.pending ? "Revealing…" : "Reveal passphrase"}
						</Button>
					</Box>
				)}

				{reveal.error && (
					<Alert severity="error">{reveal.error.message}</Alert>
				)}

				{revealed && (
					<Stack spacing={1}>
						<Alert severity="error">
							<AlertTitle>
								Save this to Bitwarden NOW — it cannot be shown again
							</AlertTitle>
							Stored under Secret <code>{revealed.repo_password_ref}</code>.
						</Alert>
						<Stack
							direction="row"
							spacing={1}
							sx={{ alignItems: "center" }}
						>
							<Box
								component="code"
								data-testid="escrow-passphrase"
								sx={{
									flex: 1,
									p: 1.5,
									fontFamily: "monospace",
									bgcolor: "action.hover",
									borderRadius: 1,
									wordBreak: "break-all",
								}}
							>
								{revealed.passphrase}
							</Box>
							<Tooltip title="Copy">
								<IconButton
									aria-label="copy passphrase"
									onClick={() =>
										navigator.clipboard?.writeText(revealed.passphrase)
									}
								>
									<ContentCopyIcon />
								</IconButton>
							</Tooltip>
						</Stack>

						<FormControlLabel
							control={
								<Checkbox
									checked={saved}
									onChange={(e) => setSaved(e.target.checked)}
								/>
							}
							label="I have saved this passphrase to Bitwarden"
						/>
						<Box>
							<Button
								variant="contained"
								color="success"
								onClick={onAck}
								disabled={!saved || ack.pending}
							>
								{ack.pending ? "Activating…" : "Acknowledge & activate backups"}
							</Button>
						</Box>
						{ack.error && (
							<Alert severity="error">{ack.error.message}</Alert>
						)}
					</Stack>
				)}
			</Stack>
		</Paper>
	);
}
