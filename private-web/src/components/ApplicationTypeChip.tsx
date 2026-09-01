import { Chip } from "@mui/material";
import { useApplicationTypeLabel } from "../hooks/useApplicationTypes";
import type { ApplicationType } from "../types";

// The software carries the colour, so a central and a facility read as two of
// the same thing at a glance. The label is what tells them apart.
const COLORS: Record<ApplicationType, "success" | "secondary" | "default"> = {
	"tamanu-central": "success",
	"tamanu-facility": "success",
	senaite: "secondary",
	canopy: "default",
};

export default function ApplicationTypeChip({
	type,
}: {
	type: ApplicationType;
}) {
	const label = useApplicationTypeLabel(type);
	return (
		<Chip size="small" variant="outlined" color={COLORS[type]} label={label} />
	);
}
