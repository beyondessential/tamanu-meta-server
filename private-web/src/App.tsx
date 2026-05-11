import {
	AppBar,
	Box,
	Container,
	Link as MuiLink,
	Toolbar,
	Typography,
} from "@mui/material";
import OpenInNewIcon from "@mui/icons-material/OpenInNew";
import { NavLink, Navigate, Route, Routes } from "react-router-dom";
import { useApi } from "./api";
import Admins from "./routes/Admins";
import Bestool from "./routes/Bestool";
import BestoolSnippetDetail from "./routes/BestoolSnippetDetail";
import BestoolSnippets from "./routes/BestoolSnippets";
import DeviceDetail from "./routes/DeviceDetail";
import Devices from "./routes/Devices";
import DevicesList from "./routes/DevicesList";
import DevicesSearch from "./routes/DevicesSearch";
import Incidents from "./routes/Incidents";
import Status from "./routes/Status";
import ServerDetail from "./routes/ServerDetail";
import ServerEdit from "./routes/ServerEdit";
import Servers from "./routes/Servers";
import ServersList from "./routes/ServersList";
import Sql from "./routes/Sql";
import VersionDetail from "./routes/VersionDetail";
import Versions from "./routes/Versions";

interface NavItem {
	label: string;
	to: string;
}

const BASE_NAV: NavItem[] = [
	{ label: "Status", to: "/status" },
	{ label: "Incidents", to: "/incidents" },
	{ label: "Servers", to: "/servers" },
	{ label: "Versions", to: "/versions" },
	{ label: "Devices", to: "/devices" },
	{ label: "Bestool", to: "/bestool" },
	{ label: "Admins", to: "/admins" },
];

export default function App() {
	const sqlAvailable = useApi<boolean>("sql", "is_sql_available");
	const publicUrl = useApi<string | null>("commons", "public_url");
	const serverVersionsUrl = useApi<string | null>("commons", "server_versions_url");
	const navItems: NavItem[] = [
		...BASE_NAV,
		...(sqlAvailable.status === "ok" && sqlAvailable.data
			? [{ label: "SQL", to: "/sql" }]
			: []),
	];

	const externalLinks: Array<{ label: string; href: string }> = [
		...(publicUrl.status === "ok" && publicUrl.data
			? [{ label: "Public", href: publicUrl.data }]
			: []),
		...(serverVersionsUrl.status === "ok" && serverVersionsUrl.data
			? [{ label: "Server Versions", href: serverVersionsUrl.data }]
			: []),
	];

	return (
		<Box>
			<AppBar position="static" color="default" elevation={1}>
				<Toolbar variant="dense" sx={{ gap: 2 }}>
					<Box
						component={NavLink}
						to="/"
						sx={{
							display: "flex",
							alignItems: "center",
							gap: 1,
							mr: 2,
							color: "primary.main",
							textDecoration: "none",
						}}
					>
						<Box
							component="img"
							src="/favicon.svg"
							alt=""
							aria-hidden
							sx={{ height: 24, width: 24 }}
						/>
						<Typography variant="h6" component="h1">
							Canopy
						</Typography>
					</Box>
					{navItems.map(({ label, to }) => (
						<Typography
							key={to}
							component={NavLink}
							to={to}
							sx={({ palette }) => ({
								textDecoration: "none",
								color: palette.text.secondary,
								fontWeight: 500,
								"&.active": { color: palette.secondary.main },
							})}
						>
							{label}
						</Typography>
					))}
					<Box sx={{ flex: 1 }} />
					{externalLinks.map(({ label, href }) => (
						<Typography
							key={label}
							component={MuiLink}
							href={href}
							target="_blank"
							rel="noopener"
							sx={({ palette }) => ({
								textDecoration: "none",
								color: palette.text.secondary,
								fontWeight: 500,
								display: "inline-flex",
								alignItems: "center",
								gap: 0.5,
							})}
						>
							{label}
							<OpenInNewIcon sx={{ fontSize: "1em" }} />
						</Typography>
					))}
				</Toolbar>
			</AppBar>
			<Container maxWidth="lg" sx={{ py: 3 }}>
				<Routes>
					<Route path="/" element={<Navigate to="/status" replace />} />
					<Route path="/status" element={<Status />} />
					<Route path="/incidents" element={<Incidents />} />
					<Route path="/admins" element={<Admins />} />
					<Route path="/versions" element={<Versions />} />
					<Route path="/versions/:version" element={<VersionDetail />} />
					<Route path="/servers" element={<Servers />}>
						<Route index element={<ServersList kind="central" />} />
						<Route
							path="facilities"
							element={<ServersList kind="facility" />}
						/>
					</Route>
					<Route path="/servers/:id" element={<ServerDetail />} />
					<Route path="/servers/:id/edit" element={<ServerEdit />} />
					<Route path="/devices" element={<Devices />}>
						<Route index element={<DevicesSearch />} />
						<Route
							path="untrusted"
							element={<DevicesList scope="untrusted" />}
						/>
						<Route
							path="trusted"
							element={<DevicesList scope="trusted" />}
						/>
					</Route>
					<Route path="/devices/:id" element={<DeviceDetail />} />
					<Route path="/bestool" element={<Bestool />}>
						<Route
							index
							element={<Navigate to="/bestool/snippets" replace />}
						/>
						<Route path="snippets" element={<BestoolSnippets />} />
						<Route
							path="snippets/:id"
							element={<BestoolSnippetDetail />}
						/>
					</Route>
					<Route path="/sql" element={<Sql />} />
				</Routes>
			</Container>
		</Box>
	);
}
