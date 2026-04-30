import { Chip } from "@mui/material";
import type { ServerRank } from "../types";

const COLORS: Record<
	ServerRank,
	"error" | "warning" | "info" | "success" | "primary"
> = {
	production: "error",
	clone: "warning",
	demo: "info",
	test: "info",
	dev: "success",
};

export default function ServerRankChip({ rank }: { rank: ServerRank }) {
	return (
		<Chip
			size="small"
			variant="outlined"
			color={COLORS[rank]}
			label={rank}
			sx={{ textTransform: "capitalize" }}
		/>
	);
}
