import { Link as MuiLink, Stack } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import type { VersionStr } from "../types";
import VersionSquare from "./VersionSquare";

interface VersionIndicatorProps {
	version: VersionStr;
	distance?: number | null;
	addLink?: boolean;
}

export default function VersionIndicator({
	version,
	distance = null,
	addLink = true,
}: VersionIndicatorProps) {
	const inner = (
		<Stack direction="row" spacing={0.5} component="span" sx={{ alignItems: "center" }}>
			<span>{version}</span>
			<VersionSquare distance={distance} />
		</Stack>
	);

	if (!addLink) {
		return inner;
	}
	return (
		<MuiLink
			component={RouterLink}
			to={`/versions/${version}`}
			underline="hover"
			color="inherit"
		>
			{inner}
		</MuiLink>
	);
}
