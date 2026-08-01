import { Chip } from "@mui/material";
import type { ServerKind } from "../types";

const COLORS: Record<ServerKind, "primary" | "info" | "default"> = {
	central: "primary",
	facility: "info",
	standalone: "default",
};

export default function ServerKindChip({ kind }: { kind: ServerKind }) {
	return (
		<Chip
			size="small"
			variant="outlined"
			color={COLORS[kind]}
			label={kind}
			sx={{ textTransform: "capitalize" }}
		/>
	);
}
