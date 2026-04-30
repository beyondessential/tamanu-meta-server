import {
	Alert,
	Box,
	LinearProgress,
	Pagination,
	Stack,
} from "@mui/material";
import { useState } from "react";
import DeviceShorty from "../components/DeviceShorty";
import { useApi } from "../api";
import type { DeviceInfoData } from "../types";

const PAGE_SIZE = 10;

export default function DevicesList({
	scope,
}: {
	scope: "trusted" | "untrusted";
}) {
	const [page, setPage] = useState(0);

	const countFn = scope === "trusted" ? "count_trusted" : "count_untrusted";
	const listFn = scope === "trusted" ? "list_trusted" : "list_untrusted";

	const count = useApi<number>("devices", countFn, {}, [scope]);
	const devices = useApi<DeviceInfoData[]>(
		"devices",
		listFn,
		{ limit: PAGE_SIZE, offset: page * PAGE_SIZE },
		[scope, page],
	);

	const total = count.status === "ok" ? count.data : 0;
	const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));

	return (
		<Stack spacing={2}>
			{devices.status === "loading" || devices.status === "idle" ? (
				<LinearProgress />
			) : devices.status === "error" ? (
				<Alert severity="error">{devices.error.message}</Alert>
			) : devices.data.length === 0 ? (
				<Alert severity="info">No devices found.</Alert>
			) : (
				<Stack spacing={1}>
					{devices.data.map((d) => (
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
