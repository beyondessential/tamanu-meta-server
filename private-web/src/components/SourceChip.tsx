import { Chip, Tooltip } from "@mui/material";

/** Chip showing an issue's `source`, with `ref` as its tooltip. Manual
 * issues are submitted with a random UUID ref that's not meaningful to
 * humans, so we suppress the tooltip in that case. */
export default function SourceChip({
	source,
	refValue,
}: {
	source: string;
	refValue: string;
}) {
	const chip = <Chip label={source} size="small" variant="outlined" />;
	if (source === "manual") return chip;
	return <Tooltip title={refValue}>{chip}</Tooltip>;
}
