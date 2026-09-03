import { Box, Link as MuiLink, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";

/// One step of a target's trail: what to call it, and where it lives.
export interface TargetPart {
	label: string;
	/// Where this part links. Omit inside a row that is already one link —
	/// nested anchors are invalid — or where the part has no page of its own.
	to?: string | null;
}

/// Renders a target as the trail that locates it: `group · machine ·
/// application`, or whichever of those the caller has.
///
/// Everything but the last part sits in muted text, because the last part is
/// the thing being named and the rest is where it is. The interpunct is the
/// convention from the design brief.
///
/// A part with no `to` is plain text, so a row wrapped in a single link passes
/// none and the whole trail clicks through to the row's own target.
/// spec: FLT#navigating-the-two-grains
export default function TargetName({
	parts,
	component = "span",
}: {
	parts: TargetPart[];
	component?: React.ElementType;
}) {
	const named = parts.filter((part) => part.label !== "");
	return (
		<Box component={component} sx={{ display: "inline" }}>
			{named.map((part, index) =>
				index === named.length - 1 ? (
					<Part key={index} part={part} />
				) : (
					<Typography
						key={index}
						component="span"
						color="text.secondary"
						sx={{ mr: 0.5 }}
					>
						<Part part={part} muted /> ·
					</Typography>
				),
			)}
		</Box>
	);
}

function Part({ part, muted }: { part: TargetPart; muted?: boolean }) {
	if (!part.to) return <>{part.label}</>;
	return (
		<MuiLink
			component={RouterLink}
			to={part.to}
			underline="hover"
			color={muted ? "inherit" : undefined}
		>
			{part.label}
		</MuiLink>
	);
}
