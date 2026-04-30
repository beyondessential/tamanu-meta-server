import {
	Alert,
	Box,
	LinearProgress,
	Pagination,
	Stack,
} from "@mui/material";
import { useState } from "react";
import ServerShorty, {
	type ServerInfo,
} from "../components/ServerShorty";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import type { ServerKind } from "../types";

const PAGE_SIZE = 10;

export default function ServersList({ kind }: { kind: ServerKind }) {
	usePageTitle(kind === "central" ? "Central servers" : "Facility servers");
	const [page, setPage] = useState(0);

	const count = useApi<number>("servers", "count_some", { kind }, [kind]);
	const servers = useApi<ServerInfo[]>(
		"servers",
		"list_some",
		{ kind, offset: page * PAGE_SIZE, limit: PAGE_SIZE },
		[kind, page],
	);

	const total = count.status === "ok" ? count.data : 0;
	const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

	return (
		<Stack spacing={2}>
			{servers.status === "loading" || servers.status === "idle" ? (
				<LinearProgress />
			) : servers.status === "error" ? (
				<Alert severity="error">{servers.error.message}</Alert>
			) : servers.data.length === 0 ? (
				<Alert severity="info">No servers found.</Alert>
			) : (
				<Stack spacing={1}>
					{servers.data.map((s) => (
						<ServerShorty key={s.id} server={s} />
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
