import { Box, Tab, Tabs } from "@mui/material";
import { Outlet, useLocation, useNavigate } from "react-router-dom";

const TABS: Array<{ value: string; label: string; to: string }> = [
	{ value: "search", label: "Search", to: "/devices" },
	{ value: "all", label: "All devices", to: "/devices/all" },
];

function valueFromPath(pathname: string): string {
	if (pathname === "/devices") return "search";
	if (pathname.startsWith("/devices/all")) return "all";
	return ""; // detail/history pages — no tab highlighted
}

export default function Devices() {
	const location = useLocation();
	const navigate = useNavigate();
	const value = valueFromPath(location.pathname);

	return (
		<Box>
			<Tabs
				value={value || false}
				onChange={(_, v) => {
					const target = TABS.find((t) => t.value === v);
					if (target) navigate(target.to);
				}}
				sx={{ mb: 2 }}
			>
				{TABS.map((t) => (
					<Tab key={t.value} value={t.value} label={t.label} />
				))}
			</Tabs>
			<Outlet />
		</Box>
	);
}
