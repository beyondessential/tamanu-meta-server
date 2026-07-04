import { Box } from "@mui/material";
import { Fragment } from "react";

/// Stringify one value from a `health[]` entry for display: strings
/// verbatim, everything else as compact JSON.
export function renderCheckValue(v: unknown): string {
	if (typeof v === "string") return v;
	if (v === null) return "null";
	return JSON.stringify(v);
}

/// The extra fields of one `health[]` entry (everything except the
/// reserved `check`/`healthy`/`result` keys), in source order.
export function checkEntryExtras(
	entry: Record<string, unknown>,
): Array<[string, unknown]> {
	return Object.entries(entry).filter(
		([k]) => k !== "check" && k !== "healthy" && k !== "result",
	);
}

/// Key/value grid of a healthcheck entry's extra fields — the canonical
/// rendering of "all the data of the healthcheck", shared by the server
/// detail checks table, the status snapshot panel, and the
/// per-healthcheck attention page.
export default function CheckExtrasList({
	extras,
}: {
	extras: Array<[string, unknown]>;
}) {
	if (extras.length === 0) return null;
	return (
		<Box
			component="dl"
			sx={{
				m: 0,
				mt: 0.5,
				display: "grid",
				gridTemplateColumns: "max-content 1fr",
				columnGap: 1.5,
				rowGap: 0.25,
				fontSize: "0.8em",
			}}
		>
			{extras.map(([k, v]) => (
				<Fragment key={k}>
					<Box component="dt" sx={{ color: "text.secondary" }}>
						{k}
					</Box>
					<Box
						component="dd"
						sx={{
							m: 0,
							fontFamily: "monospace",
							minWidth: 0,
							overflowWrap: "anywhere",
						}}
					>
						{renderCheckValue(v)}
					</Box>
				</Fragment>
			))}
		</Box>
	);
}
