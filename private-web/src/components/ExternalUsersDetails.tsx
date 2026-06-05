import { Avatar, Stack, Typography } from "@mui/material";
import PersonOutlineIcon from "@mui/icons-material/PersonOutline";
import { OperatorAvatar, connectedFor } from "./OperatorAvatars";
import type { OperatorPresence } from "../types";

/** One interactive session from the `external_users` check's `users[]`. */
export type ExternalUserSession = {
	name: string | null;
	line: string | null;
	source: string | null;
	tailscale: string | null;
	connected_since: string | null;
};

/** Pull the `users[]` sessions out of the check's extras. Returns null
 * when the shape isn't what bestool ships (callers fall back to the
 * generic key/value rendering). */
export function parseExternalUserSessions(
	extras: Array<[string, unknown]>,
): ExternalUserSession[] | null {
	const users = extras.find(([k]) => k === "users")?.[1];
	if (!Array.isArray(users)) return null;
	const sessions: ExternalUserSession[] = [];
	for (const raw of users) {
		if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
			return null;
		}
		const obj = raw as Record<string, unknown>;
		const str = (k: string) =>
			typeof obj[k] === "string" ? (obj[k] as string) : null;
		sessions.push({
			name: str("name"),
			line: str("line"),
			source: str("source"),
			tailscale: str("tailscale"),
			connected_since: str("connected_since"),
		});
	}
	return sessions;
}

/** Formatted session list for the `external_users` check, replacing the
 * raw `users` JSON dump. Sessions with a Tailscale identity show the
 * person's avatar and login; unidentified ones (local console,
 * non-Tailscale SSH) show the OS username and source. `operators`
 * provides the cached display info (name, picture), joined by login. */
export default function ExternalUsersDetails({
	sessions,
	operators,
}: {
	sessions: ExternalUserSession[];
	operators: OperatorPresence[];
}) {
	if (sessions.length === 0) {
		return (
			<Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
				No interactive sessions.
			</Typography>
		);
	}
	return (
		<Stack spacing={0.75} sx={{ mt: 0.75 }}>
			{sessions.map((s, i) => (
				<SessionRow key={i} session={s} operators={operators} />
			))}
		</Stack>
	);
}

function SessionRow({
	session: s,
	operators,
}: {
	session: ExternalUserSession;
	operators: OperatorPresence[];
}) {
	const op = s.tailscale
		? (operators.find((o) => o.login === s.tailscale) ?? {
				login: s.tailscale,
				name: null,
				profile_pic: null,
				connected_since: s.connected_since,
			})
		: null;
	const dur = connectedFor(s.connected_since);
	const meta = [
		s.line,
		// The Tailscale CGNAT source IP is redundant once we've named the
		// person behind it; for unidentified sessions it's the best lead.
		op ? null : s.source,
		dur ? `connected ${dur}` : null,
	].filter(Boolean);
	return (
		<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
			{op ? (
				// Per-session connection time in the tooltip, not the
				// operator's (possibly earlier) deduped one.
				<OperatorAvatar
					op={{ ...op, connected_since: s.connected_since }}
					size={22}
				/>
			) : (
				<Avatar sx={{ width: 22, height: 22 }}>
					<PersonOutlineIcon sx={{ fontSize: 14 }} />
				</Avatar>
			)}
			<Typography variant="body2">
				{op?.login ?? s.name ?? "unknown user"}
			</Typography>
			<Typography
				variant="caption"
				color="text.secondary"
				sx={{ fontFamily: "monospace" }}
			>
				{meta.join(" · ")}
			</Typography>
		</Stack>
	);
}
