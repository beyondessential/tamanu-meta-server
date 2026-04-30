import { Chip } from "@mui/material";
import type { VersionStatus } from "../types";

const STYLES: Record<
	VersionStatus,
	{ color: "default" | "warning" | "success" | "error"; label: string }
> = {
	draft: { color: "warning", label: "Draft" },
	published: { color: "success", label: "Published" },
	yanked: { color: "error", label: "Yanked" },
};

export default function VersionStatusChip({
	status,
}: {
	status: VersionStatus;
}) {
	const { color, label } = STYLES[status];
	return <Chip size="small" color={color} label={label} variant="outlined" />;
}
