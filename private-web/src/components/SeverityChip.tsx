import { Chip } from "@mui/material";
import type { Severity } from "../types";

type Color = "error" | "warning" | "info" | "default";

const COLOR: Record<Severity, Color> = {
	emergency: "error",
	alert: "error",
	critical: "error",
	error: "error",
	warning: "warning",
	notice: "info",
	info: "info",
	debug: "default",
};

export default function SeverityChip({ severity }: { severity: Severity }) {
	return <Chip label={severity} color={COLOR[severity]} size="small" />;
}
