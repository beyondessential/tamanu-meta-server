import { Chip, Tooltip, useTheme } from "@mui/material";
import { CHECK_RESULT_INTENT, type CheckResult } from "../types";

type Color = "error" | "warning" | "success" | "default";

/// Passed/skipped read calm; failed is error, warning is amber. Broken is
/// handled separately (see below) so it never reads as a plain warning.
const COLOR: Record<Exclude<CheckResult, "broken">, Color> = {
	failed: "error",
	warning: "warning",
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
	const theme = useTheme();

	// Broken says nothing about the system under test — only that the
	// check itself couldn't run — but it isn't the same as a warning, so
	// give it a distinct hazard-stripe identity rather than reusing amber.
	// The stripes alternate amber with a darker amber (not black) so the
	// label stays legible over them; the tooltip + label carry the meaning
	// (status is never colour-alone).
	if (result === "broken") {
		const amber = theme.palette.warning.main;
		const bar = theme.palette.grey[900];
		return (
			<Tooltip title={CHECK_RESULT_INTENT.broken} arrow>
				<Chip
					label="broken"
					size="small"
					variant={variant}
					sx={{
						// Classic hazard tape: amber and charcoal diagonals.
						// A white, bold label with a dark halo stays legible
						// over both stripe colours; the tooltip + label carry
						// the meaning (status is never colour-alone).
						color: theme.palette.common.white,
						fontWeight: 700,
						border: `1px solid ${bar}`,
						backgroundColor: "transparent",
						backgroundImage: `repeating-linear-gradient(-45deg, ${amber}, ${amber} 6px, ${bar} 6px, ${bar} 12px)`,
						"& .MuiChip-label": {
							textShadow: "0 1px 2px rgba(0,0,0,0.85)",
						},
					}}
				/>
			</Tooltip>
		);
	}

	return (
		<Tooltip title={CHECK_RESULT_INTENT[result]} arrow>
			<Chip label={result} color={COLOR[result]} size="small" variant={variant} />
		</Tooltip>
	);
}
