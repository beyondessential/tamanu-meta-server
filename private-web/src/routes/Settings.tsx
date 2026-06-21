import { Box, Tab, Tabs } from "@mui/material";
import { Outlet, useLocation, useNavigate } from "react-router-dom";

/// Settings groups the infrequently-used admin pages (operator admins, recovery
/// vault) under one top-level nav item so they don't each hold space in the bar.
const TABS: Array<{ value: string; label: string; to: string }> = [
	{ value: "admins", label: "Admins", to: "/settings/admins" },
	{
		value: "backup-defaults",
		label: "Backup defaults",
		to: "/settings/backup-defaults",
	},
	{ value: "recovery", label: "Recovery vault", to: "/settings/recovery" },
];

function valueFromPath(pathname: string): string {
	if (pathname.startsWith("/settings/recovery")) return "recovery";
	if (pathname.startsWith("/settings/backup-defaults")) return "backup-defaults";
	if (pathname.startsWith("/settings/admins")) return "admins";
	return "";
}

export default function Settings() {
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
