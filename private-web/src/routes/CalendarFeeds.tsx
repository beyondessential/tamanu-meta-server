import BlockIcon from "@mui/icons-material/Block";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import {
	Alert,
	Box,
	Button,
	Chip,
	Dialog,
	DialogActions,
	DialogContent,
	DialogContentText,
	DialogTitle,
	IconButton,
	LinearProgress,
	Paper,
	Snackbar,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableRow,
	TextField,
	Tooltip,
	Typography,
} from "@mui/material";
import { type FormEvent, useState } from "react";
import { ApiError, callApi, useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import type { ApiResponse } from "../types";

type FeedRow = ApiResponse<"calendar_tokens", "list">[number];

/// Subscription feeds of planned upgrades. The URL carries the credential, so
/// it is shown once and ends by being revoked.
// spec: UPG#the-calendar-feed
export default function CalendarFeeds() {
	usePageTitle("Calendar feeds");
	const list = useApi("calendar_tokens", "list");
	const [name, setName] = useState("");
	const [pending, setPending] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [toast, setToast] = useState<string | null>(null);
	const [minted, setMinted] = useState<{ name: string; url: string } | null>(
		null,
	);
	const [confirmRevoke, setConfirmRevoke] = useState<FeedRow | null>(null);

	const onMint = async (e: FormEvent) => {
		e.preventDefault();
		const trimmed = name.trim();
		if (!trimmed) return setError("Feed name cannot be empty");
		setPending(true);
		setError(null);
		try {
			const res = await callApi("calendar_tokens", "mint", { name: trimmed });
			setName("");
			setMinted({ name: res.token.name, url: res.url ?? res.path });
			list.reload();
		} catch (err) {
			setError(formatError(err));
		} finally {
			setPending(false);
		}
	};

	const onRevoke = async (feed: FeedRow) => {
		setConfirmRevoke(null);
		try {
			await callApi("calendar_tokens", "revoke", { id: feed.id });
			setToast(`Feed "${feed.name}" revoked`);
			list.reload();
		} catch (err) {
			setError(formatError(err));
		}
	};

	return (
		<Stack spacing={3} data-testid="calendar-feeds">
			<Typography variant="body2" color="text.secondary">
				A read-only calendar of planned upgrades, for subscribing to from
				Google Calendar, Outlook, or Apple Calendar. It is served from the
				public API host, so a calendar service can fetch it without being on
				the tailnet. Anyone holding the URL can read it: mint one per
				subscriber so a single one can be revoked without disturbing the rest.
			</Typography>

			<Paper variant="outlined" sx={{ p: 2 }}>
				<Box component="form" onSubmit={onMint}>
					<Stack direction="row" spacing={1} sx={{ alignItems: "flex-start" }}>
						<TextField
							size="small"
							fullWidth
							label="Feed name"
							placeholder="e.g. deployments team"
							value={name}
							onChange={(e) => setName(e.target.value)}
							disabled={pending}
						/>
						<Button
							type="submit"
							variant="contained"
							disabled={pending}
							sx={{ whiteSpace: "nowrap", flexShrink: 0 }}
						>
							{pending ? "Minting…" : "Mint feed"}
						</Button>
					</Stack>
					{error && (
						<Alert severity="error" sx={{ mt: 2 }}>
							{error}
						</Alert>
					)}
				</Box>
			</Paper>

			<Box>
				<Typography variant="h6" component="h2" gutterBottom>
					Feeds
				</Typography>
				{list.status === "loading" || list.status === "idle" ? (
					<LinearProgress />
				) : list.status === "error" ? (
					<Alert severity="error">{list.error.message}</Alert>
				) : list.data.length === 0 ? (
					<Alert severity="info">No feeds minted.</Alert>
				) : (
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell>Name</TableCell>
								<TableCell>Minted by</TableCell>
								<TableCell>Last fetched</TableCell>
								<TableCell>Status</TableCell>
								<TableCell align="right" />
							</TableRow>
						</TableHead>
						<TableBody>
							{list.data.map((feed) => (
								<TableRow key={feed.id} hover data-testid="calendar-feed-row">
									<TableCell sx={{ fontFamily: "monospace" }}>
										{feed.name}
									</TableCell>
									<TableCell>{feed.created_by}</TableCell>
									<TableCell>
										{feed.last_used_at ? formatDate(feed.last_used_at) : "never"}
									</TableCell>
									<TableCell>
										{feed.revoked_at ? (
											<Chip size="small" label="revoked" />
										) : (
											<Chip size="small" color="success" label="active" />
										)}
									</TableCell>
									<TableCell align="right">
										{!feed.revoked_at && (
											<Tooltip title="Revoke">
												<IconButton
													size="small"
													aria-label={`revoke ${feed.name}`}
													onClick={() => setConfirmRevoke(feed)}
												>
													<BlockIcon fontSize="small" />
												</IconButton>
											</Tooltip>
										)}
									</TableCell>
								</TableRow>
							))}
						</TableBody>
					</Table>
				)}
			</Box>

			<Dialog open={!!minted} maxWidth="sm" fullWidth>
				<DialogTitle>Feed "{minted?.name}" minted</DialogTitle>
				<DialogContent>
					<DialogContentText sx={{ mb: 2 }}>
						Copy the URL now: it is shown once and cannot be retrieved again.
						In Google Calendar, add it under Other calendars, From URL.
					</DialogContentText>
					<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
						<TextField
							fullWidth
							size="small"
							value={minted?.url ?? ""}
							slotProps={{
								input: {
									readOnly: true,
									sx: { fontFamily: "monospace" },
								},
							}}
						/>
						<Tooltip title="Copy to clipboard">
							<IconButton
								aria-label="copy feed url"
								onClick={async () => {
									if (minted) {
										await navigator.clipboard.writeText(minted.url);
										setToast("Feed URL copied");
									}
								}}
							>
								<ContentCopyIcon />
							</IconButton>
						</Tooltip>
					</Stack>
					{minted?.url.startsWith("/") && (
						<Alert severity="warning" sx={{ mt: 2 }}>
							The public API base URL is not configured, so this is a path
							only. Prefix it with the public API host.
						</Alert>
					)}
				</DialogContent>
				<DialogActions>
					<Button onClick={() => setMinted(null)}>Done</Button>
				</DialogActions>
			</Dialog>

			<Dialog open={!!confirmRevoke} onClose={() => setConfirmRevoke(null)}>
				<DialogTitle>Revoke "{confirmRevoke?.name}"?</DialogTitle>
				<DialogContent>
					<DialogContentText>
						The URL stops serving immediately. Subscribers keep whatever their
						calendar last fetched until they remove the subscription.
					</DialogContentText>
				</DialogContent>
				<DialogActions>
					<Button onClick={() => setConfirmRevoke(null)}>Cancel</Button>
					<Button
						color="error"
						onClick={() => confirmRevoke && onRevoke(confirmRevoke)}
					>
						Revoke
					</Button>
				</DialogActions>
			</Dialog>

			<Snackbar
				open={!!toast}
				autoHideDuration={3000}
				onClose={() => setToast(null)}
				message={toast ?? ""}
			/>
		</Stack>
	);
}

function formatDate(iso: string): string {
	return new Date(iso).toLocaleDateString(undefined, {
		year: "numeric",
		month: "short",
		day: "numeric",
	});
}

function formatError(err: unknown): string {
	if (err instanceof ApiError) {
		const detail = err.detail as { title?: string } | null;
		if (detail?.title) return detail.title;
		return err.message;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}
