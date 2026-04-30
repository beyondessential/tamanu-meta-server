import { Container, Typography } from "@mui/material";
import { Route, Routes } from "react-router-dom";
import Hello from "./routes/Hello";

export default function App() {
	return (
		<Container maxWidth="md" sx={{ py: 4 }}>
			<Typography variant="h4" component="h1" gutterBottom>
				Canopy
			</Typography>
			<Routes>
				<Route path="/" element={<Hello />} />
			</Routes>
		</Container>
	);
}
