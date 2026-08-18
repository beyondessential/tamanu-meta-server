import { CssBaseline, ThemeProvider, useMediaQuery } from "@mui/material";
import { LocalizationProvider } from "@mui/x-date-pickers";
import { AdapterDayjs } from "@mui/x-date-pickers/AdapterDayjs";
import { StrictMode, useMemo } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { makeTheme } from "./theme";

function Root() {
	const prefersDark = useMediaQuery("(prefers-color-scheme: dark)");
	const theme = useMemo(
		() => makeTheme(prefersDark ? "dark" : "light"),
		[prefersDark],
	);
	return (
		<ThemeProvider theme={theme}>
			<CssBaseline />
			<LocalizationProvider dateAdapter={AdapterDayjs}>
				<BrowserRouter>
					<App />
				</BrowserRouter>
			</LocalizationProvider>
		</ThemeProvider>
	);
}

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<Root />
	</StrictMode>,
);
