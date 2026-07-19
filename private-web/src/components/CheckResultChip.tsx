import { Chip, Tooltip } from "@mui/material";
import { CHECK_RESULT_INTENT, type CheckResult } from "../types";

type Color = "error" | "warning" | "success" | "default";

/// Broken reads as a warning since it says nothing about the system
/// under test, just the check itself; passed/skipped read calm.
const COLOR: Record<CheckResult, Color> = {
	failed: "error",
	warning: "warning",
	broken: "warning",
	passed: "success",
	skipped: "default",
};

export default function CheckResultChip({
	result,
	variant,
}: {
	result: CheckResult;
	variant?: "filled" | "outlined";
}) {
	return (
		<Tooltip title={CHECK_RESULT_INTENT[result]} arrow>
			<Chip label={result} color={COLOR[result]} size="small" variant={variant} />
		</Tooltip>
	);
}
