import {
	Alert,
	Box,
	Button,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	IconButton,
	LinearProgress,
	MenuItem,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableRow,
	TextField,
	Typography,
} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import DeviceShorty from "../components/DeviceShorty";
import { callApi, useApi, useApiAction } from "../api";
import { parseCanopyTicket, type CanopyTicket } from "../lib/canopyTicket";
import type {
	DeviceInfoData,
	ServerKind,
	ServerRank,
} from "../types";

export default function DevicesSearch() {
	const isAdmin = useApi<boolean>("commons", "is_current_user_admin");
	const [query, setQuery] = useState("");
	const [results, setResults] = useState<DeviceInfoData[] | null>(null);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		if (!query.trim()) {
			setResults(null);
			setError(null);
			return;
		}
		let cancelled = false;
		setLoading(true);
		setError(null);
		(async () => {
			try {
				const found = await callApi<DeviceInfoData[]>(
					"devices",
					"search",
					{ query: query.trim() },
				);
				if (!cancelled) setResults(found);
			} catch (err) {
				if (!cancelled)
					setError(err instanceof Error ? err.message : String(err));
			} finally {
				if (!cancelled) setLoading(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [query]);

	return (
		<Stack spacing={2}>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h5" component="h2">
					Search devices
				</Typography>
				{isAdmin.status === "ok" && isAdmin.data && <ImportTicketButton />}
			</Stack>
			<TextField
				type="search"
				fullWidth
				placeholder="Search by public key, key name, or connection IP…"
				value={query}
				onChange={(e) => setQuery(e.target.value)}
			/>
			{loading && <LinearProgress />}
			{error && <Alert severity="error">{error}</Alert>}
			{!loading && results !== null && results.length === 0 && query.trim() && (
				<Alert severity="info">No devices found matching your search.</Alert>
			)}
			{results && results.length > 0 && (
				<Stack spacing={1}>
					{results.map((d) => (
						<DeviceShorty key={d.device.id} device={d} />
					))}
				</Stack>
			)}
		</Stack>
	);
}

function ImportTicketButton() {
	const [open, setOpen] = useState(false);
	return (
		<>
			<Button variant="contained" onClick={() => setOpen(true)}>
				Import Ticket
			</Button>
			{open && <ImportTicketDialog onClose={() => setOpen(false)} />}
		</>
	);
}

function ImportTicketDialog({ onClose }: { onClose: () => void }) {
	const navigate = useNavigate();
	const [ticket, setTicket] = useState("");
	const [kind, setKind] = useState<ServerKind>("facility");
	const [rank, setRank] = useState<ServerRank | "">("");
	const [error, setError] = useState<string | null>(null);
	const action = useApiAction<string>("servers", "import_ticket");

	const parsed: CanopyTicket | null = useMemo(
		() => parseCanopyTicket(ticket),
		[ticket],
	);

	useEffect(() => {
		if (parsed?.kind) setKind(parsed.kind);
		if (parsed?.rank) setRank(parsed.rank);
	}, [parsed]);

	const submit = async (e: React.FormEvent) => {
		e.preventDefault();
		const value = ticket.trim();
		if (!value) {
			setError("Ticket cannot be empty");
			return;
		}
		setError(null);
		try {
			const serverId = await action.call({
				ticket_b64: value,
				kind,
				rank: rank === "" ? null : rank,
			});
			onClose();
			navigate(`/servers/${serverId}`);
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
		}
	};

	return (
		<Dialog open onClose={onClose} fullWidth maxWidth="sm">
			<DialogTitle sx={{ pr: 6 }}>
				Import Canopy Ticket
				<IconButton
					aria-label="close"
					onClick={onClose}
					sx={{ position: "absolute", right: 8, top: 8 }}
				>
					<CloseIcon />
				</IconButton>
			</DialogTitle>
			<Box component="form" onSubmit={submit}>
				<DialogContent dividers>
					<Stack spacing={2}>
						<TextField
							label="Ticket (base64)"
							placeholder="Paste the base64-encoded Canopy Ticket here..."
							multiline
							minRows={5}
							value={ticket}
							onChange={(e) => setTicket(e.target.value)}
							slotProps={{
								input: { sx: { fontFamily: "monospace" } },
							}}
							fullWidth
						/>
						{parsed && <ParsedTicketTable ticket={parsed} />}
						<Stack direction="row" spacing={2}>
							<TextField
								select
								label="Kind"
								value={kind}
								onChange={(e) => setKind(e.target.value as ServerKind)}
								disabled={!!parsed?.kind}
							>
								<MenuItem value="facility">facility</MenuItem>
								<MenuItem value="central">central</MenuItem>
							</TextField>
							<TextField
								select
								label="Rank"
								value={rank}
								onChange={(e) =>
									setRank(e.target.value as ServerRank | "")
								}
								disabled={!!parsed?.rank}
							>
								<MenuItem value="">unranked</MenuItem>
								<MenuItem value="production">production</MenuItem>
								<MenuItem value="clone">clone</MenuItem>
								<MenuItem value="demo">demo</MenuItem>
								<MenuItem value="test">test</MenuItem>
								<MenuItem value="dev">dev</MenuItem>
							</TextField>
						</Stack>
						{error && <Alert severity="error">{error}</Alert>}
					</Stack>
				</DialogContent>
				<DialogActions>
					<Button onClick={onClose} disabled={action.pending}>
						Cancel
					</Button>
					<Button
						type="submit"
						variant="contained"
						disabled={action.pending}
					>
						{action.pending ? "Importing…" : "Import"}
					</Button>
				</DialogActions>
			</Box>
		</Dialog>
	);
}

function ParsedTicketTable({ ticket }: { ticket: CanopyTicket }) {
	const rows: Array<[string, string]> = [
		["Server ID", ticket.serverId],
		["Host", ticket.canonicalUrl],
		["Hostname", ticket.hostname],
	];
	if (ticket.tailscaleIp) rows.push(["Tailscale IP", ticket.tailscaleIp]);
	if (ticket.hosting) rows.push(["Hosting", ticket.hosting]);
	return (
		<Table size="small">
			<TableBody>
				{rows.map(([label, value]) => (
					<TableRow key={label}>
						<TableCell sx={{ fontWeight: 500, width: "10em" }}>
							{label}
						</TableCell>
						<TableCell sx={{ fontFamily: "monospace" }}>{value}</TableCell>
					</TableRow>
				))}
			</TableBody>
		</Table>
	);
}
