import {
	AppBar,
	Box,
	Container,
	Toolbar,
	Typography,
} from "@mui/material";
import { NavLink, Navigate, Route, Routes } from "react-router-dom";
import Admins from "./routes/Admins";
import Status from "./routes/Status";

const NAV_ITEMS: Array<{ label: string; to: string }> = [
	{ label: "Status", to: "/status" },
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
				</Routes>
			</Container>
		</Box>
	);
}
