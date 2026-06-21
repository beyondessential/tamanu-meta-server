import {
	Alert,
	Button,
	Chip,
	LinearProgress,
	Link as MuiLink,
	List,
	ListItem,
	ListItemText,
	Paper,
	Snackbar,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { useApi, useApiAction } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

/// Server-wide recovery vault status + verification ceremony.
///
/// Canopy backs up its recovery-critical state (every repo passphrase + repo
/// coordinates) age-encrypted to recipient public keys it never holds the
/// private half of. This page runs the ceremony that proves a private key is
/// genuinely held and a blob decrypts — due yearly, or whenever the recipient
/// set changes.
export default function RecoveryVault() {
	usePageTitle("Recovery vault");
	const status = useApi("backups", "recovery_status");
	const challenge = useApiAction("backups", "recovery_challenge");
	const verify = useApiAction("backups", "recovery_verify");

	const [ciphertext, setCiphertext] = useState<string | null>(null);
	const [answer, setAnswer] = useState("");
	const [toast, setToast] = useState<string | null>(null);

	const startChallenge = async () => {
		setAnswer("");
		const res = await challenge.call();
		setCiphertext(res.ciphertext_base64);
	};

	const submitAnswer = async () => {
		await verify.call({ answer });
		setCiphertext(null);
		setAnswer("");
		setToast("Verification recorded");
		status.reload();
	};

	if (status.status === "loading" || status.status === "idle") {
		return <LinearProgress />;
	}
	if (status.status === "error") {
		return <Alert severity="error">{status.error.message}</Alert>;
	}
	const s = status.data;

	return (
		<Stack spacing={3}>
			<Typography variant="h5" component="h1">
				Recovery vault
			</Typography>

			{!s.configured && (
				<Alert severity="warning">
					No recovery recipients are configured on this server (
					<code>CANOPY_RECOVERY_VAULT_KEYS</code>). The backups pod requires them
					and won't run without them — set them in ops before relying on the
					vault.
				</Alert>
			)}

			<Paper variant="outlined" sx={{ p: 2 }}>
				<Stack spacing={2}>
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						<Typography variant="subtitle1">Verification status</Typography>
						<Chip
							size="small"
							color={s.due ? "warning" : "success"}
							label={s.due ? "Verification due" : "Verified"}
						/>
					</Stack>
					<Typography variant="body2" color="text.secondary">
						{s.reason}.
					</Typography>
					<Typography variant="body2">
						Last verified:{" "}
						{s.last_verified_at
							? new Date(s.last_verified_at).toLocaleString()
							: "never"}
					</Typography>

					<Typography variant="subtitle2">Recipients</Typography>
					{s.recipients.length === 0 ? (
						<Typography variant="body2" color="text.secondary">
							None configured.
						</Typography>
					) : (
						<List dense disablePadding>
							{s.recipients.map((r) => (
								<ListItem key={r} disableGutters>
									<ListItemText
										slotProps={{
											primary: {
												sx: { fontFamily: "monospace", wordBreak: "break-all" },
											},
										}}
										primary={r}
									/>
								</ListItem>
							))}
						</List>
					)}
				</Stack>
			</Paper>

			<Paper variant="outlined" sx={{ p: 2 }}>
				<Stack spacing={2}>
					<Typography variant="subtitle1">
						Run the verification ceremony
					</Typography>
					<Typography variant="body2" color="text.secondary">
						Canopy issues a challenge encrypted to the recipients above. Decrypt
						it offline with one of the held private keys — e.g.{" "}
						<code>base64 -d | bestool crypto decrypt --key-path identity</code>{" "}
						(or <code>age -d</code>) — and paste the result back to confirm a
						key is genuinely held and the vault decrypts.
					</Typography>

					<Stack direction="row" spacing={1}>
						<Button
							variant="contained"
							onClick={startChallenge}
							disabled={!s.configured || challenge.pending}
						>
							{challenge.pending ? "Issuing…" : "Issue challenge"}
						</Button>
					</Stack>
					{challenge.error && (
						<Alert severity="error">{challenge.error.message}</Alert>
					)}

					{ciphertext && (
						<>
							<TextField
								label="Challenge (base64 age ciphertext)"
								value={ciphertext}
								multiline
								minRows={3}
								slotProps={{ htmlInput: { readOnly: true } }}
								sx={{ "& textarea": { fontFamily: "monospace", fontSize: 12 } }}
							/>
							<Button
								size="small"
								onClick={() => {
									navigator.clipboard?.writeText(ciphertext);
									setToast("Copied");
								}}
								sx={{ alignSelf: "flex-start" }}
							>
								Copy challenge
							</Button>
							<TextField
								label="Decrypted answer"
								value={answer}
								onChange={(e) => setAnswer(e.target.value)}
								disabled={verify.pending}
							/>
							{verify.error && (
								<Alert severity="error">{verify.error.message}</Alert>
							)}
							<Button
								variant="contained"
								onClick={submitAnswer}
								disabled={verify.pending || answer.trim() === ""}
								sx={{ alignSelf: "flex-start" }}
							>
								{verify.pending ? "Verifying…" : "Submit answer"}
							</Button>
						</>
					)}
				</Stack>
			</Paper>

			<Typography variant="caption" color="text.secondary">
				The vault is written by the backups pod to object-locked S3. Recovery
				decrypts the latest object with a held private key; see the ops runbook.{" "}
				<MuiLink
					href="https://github.com/beyondessential/bestool"
					target="_blank"
					rel="noreferrer"
				>
					bestool crypto
				</MuiLink>
				.
			</Typography>

			<Snackbar
				open={!!toast}
				autoHideDuration={2500}
				onClose={() => setToast(null)}
				message={toast ?? ""}
			/>
		</Stack>
	);
}
