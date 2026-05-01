import { Box, Tooltip } from "@mui/material";
import type { ShortStatus } from "../types";

const STATUS_COLOR: Record<ShortStatus, string> = {
	up: "success.main",
	down: "error.main",
	away: "warning.main",
	blip: "secondary.main",
	gone: "text.disabled",
};

interface StatusDotProps {
	up: ShortStatus;
	title?: string;
	dim?: boolean;
	/** Size relative to the surrounding font size. Defaults to "1em". */
	size?: string;
}

export default function StatusDot({
	up,
	title,
	dim,
	size = "1em",
}: StatusDotProps) {
	const dot = (
		<Box
			component="span"
			sx={{
				display: "inline-block",
				width: size,
				height: size,
				borderRadius: "50%",
				bgcolor: STATUS_COLOR[up],
				opacity: dim ? 0.5 : 1,
				marginRight: "0.5em",
				verticalAlign: "middle",
			}}
		/>
	);
	if (title) {
		return <Tooltip title={title}>{dot}</Tooltip>;
	}
	return dot;
}
