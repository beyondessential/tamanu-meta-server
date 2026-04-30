import { Box, Tooltip } from "@mui/material";
import type { ShortStatus } from "../types";

const STATUS_COLOR: Record<ShortStatus, string> = {
	up: "success.main",
	down: "error.main",
	away: "warning.main",
	blip: "primary.main",
	gone: "text.disabled",
};

interface StatusDotProps {
	up: ShortStatus;
	title?: string;
	dim?: boolean;
}

export default function StatusDot({ up, title, dim }: StatusDotProps) {
	const dot = (
		<Box
			component="span"
			sx={{
				display: "inline-block",
				width: "1em",
				height: "1em",
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
