import { Link as MuiLink, Stack } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import type { VersionStr, VersionTracking } from "../types";
import VersionSquare from "./VersionSquare";

interface VersionIndicatorProps {
	version: VersionStr | null;
	/// How the server's product treats versions. `absent` renders nothing at
	/// all — there is no version to learn, so an "unknown" would read as a
	/// reporting failure. `reported` renders the bare version: canopy holds no
	/// release train to measure it against, so there is nothing to grade or to
	/// link to. Only `tracked` gets the distance square and the catalogue link.
	/// `undefined` while the product catalogue is still loading, which also
	/// renders nothing — better a momentarily blank cell than an "unknown" for
	/// a server that has no version.
	// spec: APP#versions
	tracking: VersionTracking | undefined;
	distance?: number | null;
	addLink?: boolean;
}

export default function VersionIndicator({
	version,
	tracking,
	distance = null,
	addLink = true,
}: VersionIndicatorProps) {
	if (tracking === undefined || tracking === "absent") {
		return null;
	}

	if (tracking === "reported") {
		return <span>{version ?? "unknown"}</span>;
	}

	const inner = (
		<Stack direction="row" spacing={0.5} component="span" sx={{ alignItems: "center" }}>
			<span>{version ?? "unknown"}</span>
			<VersionSquare distance={distance} />
		</Stack>
	);

	if (!addLink || !version) {
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
