import { Box, Typography } from "@mui/material";

/**
 * Renders `<group name> · <server name>` in lists/headers. The group name
 * sits in muted text, the interpunct is the convention from the design
 * brief, and the server name uses the surrounding typography. When the
 * server is ungrouped, the group prefix is skipped entirely — no leading
 * separator.
 */
export default function ServerNameWithGroup({
	groupName,
	serverName,
	component = "span",
}: {
	groupName?: string | null;
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
				{groupName} ·
			</Typography>
			{serverName}
		</Box>
	);
}
