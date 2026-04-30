import { Box, Chip, Link as MuiLink, Stack } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";
import type { DeviceInfoData, DeviceRole } from "../types";

const ROLE_COLORS: Record<
	DeviceRole,
	"error" | "primary" | "warning" | "info"
> = {
	untrusted: "error",
	server: "primary",
	releaser: "warning",
	admin: "info",
};

export function deviceDisplayName(info: DeviceInfoData): string {
	const namedKey = info.keys.findLast(
		(k) => k.name && k.name !== "Initial Key",
	);
	if (namedKey?.name) return namedKey.name;
	if (info.latest_connection) return info.latest_connection.ip;
	return info.device.id;
}

export default function DeviceShorty({ device }: { device: DeviceInfoData }) {
	const name = deviceDisplayName(device);
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
				to={`/devices/${device.device.id}`}
				underline="hover"
				color="text.primary"
				sx={{ fontWeight: 500 }}
			>
				{name}
			</MuiLink>
			<Box sx={{ ml: "auto" }}>
				<Chip
					size="small"
					variant="outlined"
					color={ROLE_COLORS[device.device.role]}
					label={device.device.role}
					sx={{ textTransform: "capitalize" }}
				/>
			</Box>
		</Stack>
	);
}
