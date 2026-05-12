import { Avatar, Tooltip } from "@mui/material";

/** Small circular avatar for a Tailscale user. Falls back to the first
 * letter of the login or name when no profile picture is available. */
export default function UserAvatar({
	login,
	name,
	profilePic,
	size = 24,
}: {
	login: string | null;
	name?: string | null;
	profilePic?: string | null;
	size?: number;
}) {
	const display = name ?? login ?? "?";
	const initial = (display.trim()[0] ?? "?").toUpperCase();
	return (
		<Tooltip title={display}>
			<Avatar
				src={profilePic ?? undefined}
				alt={display}
				sx={{ width: size, height: size, fontSize: size * 0.55 }}
			>
				{initial}
			</Avatar>
		</Tooltip>
	);
}
