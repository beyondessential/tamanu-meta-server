import { Box, Link as MuiLink, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";

/**
 * Renders `<group name> · <server name>` in lists/headers. The group name
 * sits in muted text, the interpunct is the convention from the design
 * brief, and the server name uses the surrounding typography. When the
 * server is ungrouped, the group prefix is skipped entirely — no leading
 * separator.
 *
 * Pass `groupId` to make the group name a link to the group page. Leave
 * it off in contexts that are already wrapped in a link (list rows) —
 * nested anchors are invalid.
 */
export default function ServerNameWithGroup({
	groupName,
	groupId,
	serverName,
	component = "span",
}: {
	groupName?: string | null;
	groupId?: string | null;
	serverName: string;
	component?: React.ElementType;
}) {
	if (!groupName) {
		return (
			<Box component={component} sx={{ display: "inline" }}>
				{serverName}
			</Box>
		);
	}
	return (
		<Box component={component} sx={{ display: "inline" }}>
			<Typography
				component="span"
				color="text.secondary"
				sx={{ mr: 0.5 }}
			>
				{groupId ? (
					<MuiLink
						component={RouterLink}
						to={`/groups/${groupId}`}
						color="inherit"
						underline="hover"
					>
						{groupName}
					</MuiLink>
				) : (
					groupName
				)}{" "}
				·
			</Typography>
			{serverName}
		</Box>
	);
}
