import { Box, Tab, Tabs, Typography } from "@mui/material";
import { Outlet, useLocation, useNavigate, useParams } from "react-router-dom";
import { useApi } from "../api";

export default function Servers() {
	const location = useLocation();
	const navigate = useNavigate();
	const params = useParams<{ id?: string }>();

	const value = location.pathname.startsWith("/servers/ungrouped")
		? "ungrouped"
		: "groups";

	return (
		<Box>
			<Tabs
				value={value}
				onChange={(_, v) =>
					navigate(v === "groups" ? "/servers" : "/servers/ungrouped")
				}
				sx={{ mb: 2 }}
			>
				<Tab label="Groups" value="groups" />
				<Tab label="Ungrouped" value="ungrouped" />
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
