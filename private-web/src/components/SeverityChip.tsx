import { Chip, Tooltip } from "@mui/material";
import { SEVERITY_INTENT, type Severity } from "../types";

type Color = "error" | "warning" | "info" | "default";

const COLOR: Record<Severity, Color> = {
	critical: "error",
	error: "error",
	warning: "warning",
	info: "info",
	debug: "default",
};

export default function SeverityChip({ severity }: { severity: Severity }) {
	return (
		<Tooltip title={SEVERITY_INTENT[severity]} arrow>
			<Chip label={severity} color={COLOR[severity]} size="small" />
		</Tooltip>
	);
}
