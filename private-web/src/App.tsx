import {
	AppBar,
	Box,
	Container,
	Toolbar,
	Typography,
} from "@mui/material";
import { NavLink, Navigate, Route, Routes } from "react-router-dom";
import Admins from "./routes/Admins";
import DeviceDetail from "./routes/DeviceDetail";
import Devices from "./routes/Devices";
import DevicesList from "./routes/DevicesList";
import DevicesSearch from "./routes/DevicesSearch";
import Status from "./routes/Status";
import ServerDetail from "./routes/ServerDetail";
import ServerEdit from "./routes/ServerEdit";
import Servers from "./routes/Servers";
import ServersList from "./routes/ServersList";
import VersionDetail from "./routes/VersionDetail";
import Versions from "./routes/Versions";

const NAV_ITEMS: Array<{ label: string; to: string }> = [
	{ label: "Status", to: "/status" },
	{ label: "Servers", to: "/servers" },
	{ label: "Versions", to: "/versions" },
	{ label: "Devices", to: "/devices" },
	{ label: "Admins", to: "/admins" },
];

export default function App() {
	return (
		<Box>
			<AppBar position="static" color="default" elevation={1}>
				<Toolbar variant="dense" sx={{ gap: 2 }}>
					<Typography variant="h6" component="h1" sx={{ mr: 2 }}>
						Canopy
					</Typography>
					{NAV_ITEMS.map(({ label, to }) => (
						<Typography
							key={to}
							component={NavLink}
							to={to}
							sx={({ palette }) => ({
								textDecoration: "none",
								color: palette.text.secondary,
								fontWeight: 500,
								"&.active": { color: palette.text.primary },
							})}
						>
							{label}
						</Typography>
					))}
				</Toolbar>
			</AppBar>
			<Container maxWidth="lg" sx={{ py: 3 }}>
				<Routes>
					<Route path="/" element={<Navigate to="/status" replace />} />
					<Route path="/status" element={<Status />} />
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
				</Routes>
			</Container>
		</Box>
	);
}
