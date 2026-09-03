import { Box, Chip, Link as MuiLink, Stack, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import type { ServerGroup } from "../types";

export default function GroupShorty({
	group,
	memberCount,
}: {
	group: ServerGroup;
	memberCount?: number;
}) {
	const tagCount = Object.keys(group.tags ?? {}).length;
	const notesPreview = group.notes ? group.notes.split("\n")[0] : null;
	return (
		<Stack
			direction="row"
			spacing={2}
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
				alignItems: "center",
			}}
		>
			<MuiLink
				component={RouterLink}
				to={`/fleet/groups/${group.id}`}
				underline="hover"
				color="text.primary"
				sx={{ fontWeight: 500 }}
			>
				{group.name}
			</MuiLink>
			{typeof memberCount === "number" && (
				<Chip
					size="small"
					variant="outlined"
					label={`${memberCount} server${memberCount === 1 ? "" : "s"}`}
				/>
			)}
			{tagCount > 0 && (
				<Chip
					size="small"
					variant="outlined"
					label={`${tagCount} tag${tagCount === 1 ? "" : "s"}`}
				/>
			)}
			<Box sx={{ ml: "auto", minWidth: 0 }}>
				{notesPreview && (
					<Typography
						variant="body2"
						color="text.secondary"
						sx={{
							overflow: "hidden",
							textOverflow: "ellipsis",
							whiteSpace: "nowrap",
							maxWidth: "40ch",
						}}
					>
						{notesPreview}
					</Typography>
				)}
			</Box>
		</Stack>
	);
}
