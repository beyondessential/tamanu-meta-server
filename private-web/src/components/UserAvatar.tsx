import { Avatar, Tooltip } from "@mui/material";

/** Small circular avatar for a Tailscale user. Falls back to the first
 * letter of the login or name when no profile picture is available.
 *
 * Pass `tooltip={false}` when a parent already wraps this in its own tooltip,
 * to avoid stacking two tooltips on the same element. */
export default function UserAvatar({
	login,
	name,
	profilePic,
	size = 24,
	tooltip = true,
}: {
	login: string | null;
	name?: string | null;
	profilePic?: string | null;
	size?: number;
	tooltip?: boolean;
}) {
	const display = name ?? login ?? "?";
	const initial = (display.trim()[0] ?? "?").toUpperCase();
	const avatar = (
		<Avatar
			src={profilePic ?? undefined}
			alt={display}
			sx={{ width: size, height: size, fontSize: size * 0.55 }}
		>
			{initial}
		</Avatar>
	);
	if (!tooltip) return avatar;
	return <Tooltip title={display}>{avatar}</Tooltip>;
}
