import {
	Alert,
	Box,
	Button,
	LinearProgress,
	Pagination,
	Stack,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import { useState } from "react";
import DeviceShorty from "../components/DeviceShorty";
import ProvisionCredentialDialog from "../components/ProvisionCredentialDialog";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";

const PAGE_SIZE = 10;

export default function DevicesList({
	scope,
}: {
	scope: "trusted" | "untrusted";
}) {
	usePageTitle(scope === "trusted" ? "Trusted devices" : "Untrusted devices");
	const [page, setPage] = useState(0);
	const [createOpen, setCreateOpen] = useState(false);

	const listFn = scope === "trusted" ? "list_trusted" : "list_untrusted";
	const result = useApi(
		"devices",
		listFn,
		{ offset: page * PAGE_SIZE, limit: PAGE_SIZE },
		[scope, page],
	);

	const total = result.status === "ok" ? result.data.total : 0;
	const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

	return (
		<Stack spacing={2}>
			{scope === "trusted" && (
				<Box sx={{ display: "flex", justifyContent: "flex-end" }}>
					<Button
						variant="contained"
						startIcon={<AddIcon />}
						onClick={() => setCreateOpen(true)}
					>
						Create device
					</Button>
					<ProvisionCredentialDialog
						open={createOpen}
						onClose={() => setCreateOpen(false)}
						onProvisioned={() => result.reload()}
					/>
				</Box>
			)}
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : result.data.items.length === 0 ? (
				<Alert severity="info">No devices found.</Alert>
			) : (
				<Stack spacing={1}>
					{result.data.items.map((d) => (
						<DeviceShorty key={d.device.id} device={d} />
					))}
				</Stack>
			)}
			{pageCount > 1 && (
				<Box sx={{ display: "flex", justifyContent: "center" }}>
					<Pagination
						count={pageCount}
						page={page + 1}
						onChange={(_, p) => setPage(p - 1)}
					/>
				</Box>
			)}
		</Stack>
	);
}
