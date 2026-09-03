import { Box, Tab, Tabs, Typography } from "@mui/material";
import { Outlet, useLocation, useNavigate, useParams } from "react-router-dom";
import { useApi } from "../api";

export default function Servers() {
	const location = useLocation();
	const navigate = useNavigate();
	const params = useParams<{ id?: string }>();

	// No ungrouped tab: a machine is created in a group and an application takes
	// its machine's, so there is nothing left for that listing to list.
	// spec: FLT
	const value = location.pathname.startsWith("/servers/archived")
		? "archived"
		: location.pathname.startsWith("/servers/figures")
			? "figures"
			: "groups";

	const tabTarget: Record<string, string> = {
		groups: "/servers",
		archived: "/servers/archived",
		figures: "/servers/figures",
	};

	return (
		<Box>
			<Tabs
				value={value}
				onChange={(_, v) => navigate(tabTarget[v])}
				sx={{ mb: 2 }}
			>
				<Tab label="Groups" value="groups" />
				<Tab label="Archived" value="archived" />
				<Tab label="Figures" value="figures" />
			</Tabs>
			{params.id && <ServerNameBreadcrumb serverId={params.id} />}
			<Outlet />
		</Box>
	);
}

function ServerNameBreadcrumb({ serverId }: { serverId: string }) {
	const result = useApi(
		"applications",
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
