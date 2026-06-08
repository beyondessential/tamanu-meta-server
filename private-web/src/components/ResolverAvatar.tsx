import MonitorHeartOutlinedIcon from "@mui/icons-material/MonitorHeartOutlined";
import { Avatar, Tooltip } from "@mui/material";
import { AUTOMATION_RESOLVER_LABEL } from "../types";
import UserAvatar from "./UserAvatar";

/** Avatar for whoever (or whatever) resolved an issue/incident.
 *
 * When an operator login is attached, this is their {@link UserAvatar}. When
 * it's absent the incident retired on its own — the healthcheck started
 * reporting healthy again — so we show a heartbeat icon and say so, rather
 * than a blank "?". The label describes the event, not an actor: it does not
 * claim nobody intervened, only that canopy can't attribute the close. */
export default function ResolverAvatar({
	resolvedBy,
	resolvedByName,
	resolvedByPic,
	resolvedReason,
	size = 24,
}: {
	resolvedBy: string | null;
	resolvedByName?: string | null;
	resolvedByPic?: string | null;
	resolvedReason?: string | null;
	size?: number;
}) {
	const reasonPart = resolvedReason ? `(${resolvedReason}) ` : "";

	if (resolvedBy == null) {
		return (
			<Tooltip title={`resolved ${reasonPart}by ${AUTOMATION_RESOLVER_LABEL}`}>
				<Avatar
					sx={{
						width: size,
						height: size,
						bgcolor: "action.selected",
						color: "text.secondary",
					}}
				>
					<MonitorHeartOutlinedIcon sx={{ fontSize: size * 0.6 }} />
				</Avatar>
			</Tooltip>
		);
	}

	return (
		<Tooltip title={`resolved ${reasonPart}by ${resolvedByName ?? resolvedBy}`}>
			<span>
				<UserAvatar
					login={resolvedBy}
					name={resolvedByName}
					profilePic={resolvedByPic}
					size={size}
					tooltip={false}
				/>
			</span>
		</Tooltip>
	);
}
