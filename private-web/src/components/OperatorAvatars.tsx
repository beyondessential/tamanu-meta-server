import { Avatar, AvatarGroup, Tooltip } from "@mui/material";
import { humanSeconds } from "../lib/humanDuration";
import type { OperatorPresence } from "../types";

/** "3h 12m"-style coarse duration since `sinceIso`, or null when the
 * timestamp is absent/unparseable. */
export function connectedFor(sinceIso: string | null): string | null {
	if (!sinceIso) return null;
	const ms = Date.now() - Date.parse(sinceIso);
	if (Number.isNaN(ms)) return null;
	return humanSeconds(Math.max(0, Math.round(ms / 1000)));
}

export function operatorTooltip(op: OperatorPresence): string {
	const who = op.name ? `${op.name} (${op.login})` : op.login;
	const dur = connectedFor(op.connected_since);
	return dur ? `${who} — connected ${dur}` : who;
}

/** One operator's avatar: Tailscale profile picture when the
 * `tailscale_users` cache knows the login, first letter of the email
 * otherwise. */
export function OperatorAvatar({
	op,
	title,
	size,
}: {
	op: OperatorPresence;
	title?: string;
	size?: number;
}) {
	return (
		<Tooltip title={title ?? operatorTooltip(op)}>
			<Avatar
				src={op.profile_pic ?? undefined}
				alt={op.name ?? op.login}
				sx={size ? { width: size, height: size, fontSize: size / 2 } : undefined}
			>
				{op.login.charAt(0).toUpperCase()}
			</Avatar>
		</Tooltip>
	);
}

/** Compact strip of operator avatars. */
export default function OperatorAvatars({
	operators,
	size = 28,
}: {
	operators: OperatorPresence[];
	size?: number;
}) {
	return (
		<AvatarGroup
			max={8}
			sx={{
				"& .MuiAvatar-root": {
					width: size,
					height: size,
					fontSize: size / 2,
				},
			}}
		>
			{operators.map((op) => (
				<OperatorAvatar key={op.login} op={op} />
			))}
		</AvatarGroup>
	);
}
