import {
	Alert,
	Box,
	Button,
	LinearProgress,
	Paper,
	Stack,
	Typography,
} from "@mui/material";
import NotificationsActiveOutlinedIcon from "@mui/icons-material/NotificationsActiveOutlined";
import { useState } from "react";
import { useApi, useApiAction } from "../api";
import { useIsAdmin } from "../hooks/useIsAdmin";
import TimeAgo from "./TimeAgo";

/// Listed at the bottom of the server / group detail page. Renders the
/// `(source, ref)` tuples that the operator has silenced at that scope and
/// offers an un-silence button per row. Silenced refs don't contribute to
/// incidents — see `database::silenced_refs` for the matching backend.
export default function SilencedRefsSection({
	scope,
	id,
}: {
	scope: "server" | "group";
	id: string;
}) {
	const isAdmin = useIsAdmin() === true;
	const [tick, setTick] = useState(0);
	const reload = () => setTick((t) => t + 1);

	const result =
		scope === "server"
			? useApi(
					"silenced_refs",
					"list_for_server",
					{ server_id: id },
					[id, tick],
				)
			: useApi(
					"silenced_refs",
					"list_for_group",
					{ server_group_id: id },
					[id, tick],
				);

	if (result.status === "loading" || result.status === "idle") {
		return (
			<Paper variant="outlined" sx={{ p: 2 }}>
				<SectionHeading scope={scope} />
				<LinearProgress />
			</Paper>
		);
	}
	if (result.status === "error") {
		return (
			<Paper variant="outlined" sx={{ p: 2 }}>
				<SectionHeading scope={scope} />
				<Alert severity="error">{result.error.message}</Alert>
			</Paper>
		);
	}
	const rows = result.data;
	if (rows.length === 0) {
		return null;
	}

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<SectionHeading scope={scope} />
			<Stack spacing={1}>
				{rows.map((row) => (
					<SilencedRow
						key={`${row.source}\x00${row.ref}`}
						scope={scope}
						id={id}
						source={row.source}
						refName={row.ref}
						createdAt={row.created_at}
						createdBy={row.created_by ?? null}
						isAdmin={isAdmin}
						onChanged={reload}
					/>
				))}
			</Stack>
		</Paper>
	);
}

function SectionHeading({ scope }: { scope: "server" | "group" }) {
	return (
		<Typography variant="h6" component="h2" gutterBottom>
			Silenced refs
			<Typography
				component="span"
				variant="body2"
				color="text.secondary"
				sx={{ ml: 1 }}
			>
				{scope === "server"
					? "— issues with these refs on this server don't open incidents."
					: "— issues with these refs anywhere in this group don't open incidents."}
			</Typography>
		</Typography>
	);
}

function SilencedRow({
	scope,
	id,
	source,
	refName,
	createdAt,
	createdBy,
	isAdmin,
	onChanged,
}: {
	scope: "server" | "group";
	id: string;
	source: string;
	refName: string;
	createdAt: string;
	createdBy: string | null;
	isAdmin: boolean;
	onChanged: () => void;
}) {
	const unsilenceServer = useApiAction("silenced_refs", "unsilence_server");
	const unsilenceGroup = useApiAction("silenced_refs", "unsilence_group");
	const pending =
		scope === "server" ? unsilenceServer.pending : unsilenceGroup.pending;
	const error =
		scope === "server" ? unsilenceServer.error : unsilenceGroup.error;
	const unsilence = async () => {
		try {
			if (scope === "server") {
				await unsilenceServer.call({
					server_id: id,
					source,
					ref: refName,
				});
			} else {
				await unsilenceGroup.call({
					server_group_id: id,
					source,
					ref: refName,
				});
			}
			onChanged();
		} catch {
			/* surfaced via error */
		}
	};

	return (
		<Box
			sx={{
				p: 1.5,
				border: 1,
				borderColor: "divider",
				borderRadius: 1,
			}}
		>
			<Stack
				direction="row"
				spacing={1}
				sx={{ alignItems: "center", flexWrap: "wrap" }}
				useFlexGap
			>
				<Typography
					component="code"
					sx={{ fontFamily: "monospace", fontWeight: 500 }}
				>
					{source}/{refName}
				</Typography>
				<Box sx={{ flex: 1 }} />
				<Typography variant="caption" color="text.secondary">
					silenced <TimeAgo timestamp={createdAt} />
					{createdBy && ` by ${createdBy}`}
				</Typography>
				{isAdmin && (
					<Button
						size="small"
						variant="outlined"
						startIcon={<NotificationsActiveOutlinedIcon />}
						disabled={pending}
						onClick={unsilence}
					>
						Un-silence
					</Button>
				)}
			</Stack>
			{error && <Alert severity="error" sx={{ mt: 1 }}>{error.message}</Alert>}
		</Box>
	);
}
