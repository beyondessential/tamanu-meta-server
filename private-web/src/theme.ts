import { createTheme } from "@mui/material/styles";

// Brand palette pulled from the original favicon: a sky-blue plus on a yellow
// ring. We've swapped the yellow for a leaf green and kept the blue as-is.
const LEAF = "#388e3c"; // material green-700
const LEAF_LIGHT = "#66bb6a"; // material green-400
const SKY = "#32669a"; // from favicon
const SKY_LIGHT = "#5b8cbe";

export function makeTheme(mode: "light" | "dark") {
	const dark = mode === "dark";
	return createTheme({
		palette: {
			mode,
			primary: { main: dark ? LEAF_LIGHT : LEAF },
			secondary: { main: dark ? SKY_LIGHT : SKY },
		},
	});
}
