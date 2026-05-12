import { Box, Tab, Tabs, Typography } from "@mui/material";
import {
	Outlet,
	useLocation,
	useNavigate,
	useParams,
} from "react-router-dom";
import { useApi } from "../api";

export default function Servers() {
	const location = useLocation();
	const navigate = useNavigate();
	const params = useParams<{ id?: string }>();

	const value = location.pathname.startsWith("/servers/facilities")
		? "facilities"
		: "centrals";

	return (
		<Box>
			<Tabs
				value={value}
				onChange={(_, v) =>
					navigate(v === "centrals" ? "/servers" : "/servers/facilities")
				}
				sx={{ mb: 2 }}
			>
				<Tab label="Central servers" value="centrals" />
				<Tab label="Facility servers" value="facilities" />
			</Tabs>
			{params.id && <ServerNameBreadcrumb serverId={params.id} />}
			<Outlet />
		</Box>
	);
}

function ServerNameBreadcrumb({ serverId }: { serverId: string }) {
	const result = useApi(
		"servers",
		"get_name",
		{ server_id: serverId },
		[serverId],
	);
	if (result.status !== "ok") return null;
	return (
		<Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
			{result.data}
		</Typography>
	);
}
