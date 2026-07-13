import HelpOutlinedIcon from "@mui/icons-material/HelpOutlined";
import {
	Box,
	IconButton,
	LinearProgress,
	Link as MuiLink,
	Popover,
	Tooltip,
	Typography,
} from "@mui/material";
import { useState } from "react";
import { Link as RouterLink } from "react-router-dom";
import { useApi } from "../api";
import Markdown from "./Markdown";

/** `?` affordance shown next to a check name wherever its state is
 * presented: opens a popover with the check's rendered operator
 * documentation (see the healthcheck settings page, where it's
 * authored). Renders unconditionally — an undocumented check pops a
 * prompt to write the missing document instead of hiding the icon,
 * which would make documented and undocumented checks look different
 * for no discoverable reason. */
export default function CheckDocButton({
	source,
	check,
}: {
	/** The source whose catalog entry to prefer when several sources
	 * report the same check name. */
	source?: string;
	check: string;
}) {
	const [anchor, setAnchor] = useState<HTMLElement | null>(null);
	return (
		<>
			<Tooltip title={`About the ${check} check`} arrow>
				<IconButton
					size="small"
					aria-label={`Documentation for ${check}`}
					sx={{ p: 0.25 }}
					onClick={(e) => {
						e.stopPropagation();
						setAnchor(e.currentTarget);
					}}
				>
					<HelpOutlinedIcon sx={{ fontSize: 16 }} />
				</IconButton>
			</Tooltip>
			<Popover
				open={anchor !== null}
				anchorEl={anchor}
				onClose={() => setAnchor(null)}
				anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
				// Card rows toggle on click; don't let clicks inside the
				// popover bubble back into the row underneath.
				onClick={(e) => e.stopPropagation()}
				slotProps={{ paper: { sx: { maxWidth: 480, p: 2 } } }}
			>
				{anchor !== null && <DocContent source={source} check={check} />}
			</Popover>
		</>
	);
}

/** Mounted only while the popover is open, so the catalog fetch is
 * lazy: nothing is requested until the operator first asks. */
function DocContent({ source, check }: { source?: string; check: string }) {
	const list = useApi("healthchecks", "list");
	if (list.status === "loading" || list.status === "idle") {
		return <LinearProgress sx={{ width: 200 }} />;
	}
	if (list.status === "error") {
		return (
			<Typography variant="body2" color="error">
				{list.error.message}
			</Typography>
		);
	}
	// Prefer the reporting source's entry; else any documented one (the
	// same rule as the attention page when several sources report a
	// check by the same name).
	const rows = list.data.filter((r) => r.check_name === check);
	const documentation =
		rows.find((r) => r.source === source)?.documentation ??
		rows.find((r) => r.documentation)?.documentation ??
		null;
	return (
		<Box>
			{documentation ? (
				<Markdown>{documentation}</Markdown>
			) : (
				<Typography variant="body2" color="text.secondary">
					Nobody has documented this check yet.
				</Typography>
			)}
			<Typography variant="caption" sx={{ display: "block", mt: 1 }}>
				<MuiLink
					component={RouterLink}
					to={`/settings/healthchecks/${encodeURIComponent(check)}`}
				>
					{documentation ? "Edit documentation" : "Write it"}
				</MuiLink>
			</Typography>
		</Box>
	);
}
