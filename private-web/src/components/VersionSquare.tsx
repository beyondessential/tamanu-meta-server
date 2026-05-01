import { Box, Tooltip } from "@mui/material";

interface VersionSquareProps {
	distance: number | null;
}

function distanceColor(distance: number | null): string {
	if (distance == null) return "text.disabled";
	if (distance < 2) return "success.main";
	if (distance >= 10) return "error.main";
	if (distance >= 5) return "warning.main";
	return "secondary.main";
}

export default function VersionSquare({ distance }: VersionSquareProps) {
	const title =
		distance == null ? "Unknown version" : `${distance} versions behind latest`;
	return (
		<Tooltip title={title}>
			<Box
				component="span"
				sx={{
					display: "inline-block",
					width: "1em",
					height: "1em",
					borderRadius: "0.15rem",
					bgcolor: distanceColor(distance),
					marginLeft: "0.4em",
					verticalAlign: "middle",
				}}
			/>
		</Tooltip>
	);
}
