import {
	Alert,
	Box,
	LinearProgress,
	Pagination,
	Stack,
} from "@mui/material";
import { useState } from "react";
import ServerShorty from "../components/ServerShorty";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import type { ServerKind } from "../types";

const PAGE_SIZE = 10;

export default function ServersList({ kind }: { kind: ServerKind }) {
	usePageTitle(kind === "central" ? "Central servers" : "Facility servers");
	const [page, setPage] = useState(0);

	const result = useApi(
		"servers",
		"list_some",
		{ kind, offset: page * PAGE_SIZE, limit: PAGE_SIZE },
		[kind, page],
	);

	const total = result.status === "ok" ? result.data.total : 0;
	const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

	return (
		<Stack spacing={2}>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : result.data.items.length === 0 ? (
				<Alert severity="info">No servers found.</Alert>
			) : (
				<Stack spacing={1}>
					{result.data.items.map((s) => (
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
