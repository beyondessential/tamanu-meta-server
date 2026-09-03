import { Box, Link as MuiLink, Stack, Typography } from "@mui/material";
import { Link as RouterLink } from "react-router-dom";

import {
	type GroupMachine,
	groupServersByRank,
	rankMachines,
	type ServerInfo,
} from "../types";
import ServerShorty from "./ServerShorty";

/// The group as an operator navigates it: rank, then the boxes at that rank,
/// then the workloads on each box.
///
/// The group page and both detail pages end with this, so an operator learns
/// one arrangement and reads it everywhere, and moving sideways never goes back
/// through the group. Whichever page it is rendered on is marked in place
/// rather than omitted, so the tree reads as a map.
/// spec: FLT
export default function GroupTree({
	machines,
	applications,
	currentMachineId,
	currentApplicationId,
}: {
	machines: GroupMachine[];
	applications: ServerInfo[];
	/// The machine whose page this is, if any.
	currentMachineId?: string;
	/// The application whose page this is, if any.
	currentApplicationId?: string;
}) {
	const ranked = rankMachines(machines, applications);

	return (
		<Stack spacing={2} data-testid="group-tree">
			{groupServersByRank(ranked).map(([rank, boxes]) => (
				<Box key={rank ?? "_unranked"}>
					<Typography
						variant="overline"
						color="text.secondary"
						sx={{ display: "block", mb: 0.5 }}
					>
						{rank ?? "unranked"}
					</Typography>
					<Stack spacing={1.5}>
						{boxes.map((box) => (
							<Box key={box.machine.id}>
								{box.machine.id === currentMachineId ? (
									<Typography
										variant="body2"
										color="text.secondary"
										sx={{ fontWeight: 500 }}
									>
										{box.machine.name ?? "Unnamed machine"}
									</Typography>
								) : (
									<MuiLink
										component={RouterLink}
										to={`/machines/${box.machine.id}`}
										variant="body2"
										sx={{ fontWeight: 500 }}
									>
										{box.machine.name ?? "Unnamed machine"}
									</MuiLink>
								)}
								{box.applications.length === 0 ? (
									<Typography
										variant="body2"
										color="text.secondary"
										sx={{ mt: 0.5 }}
									>
										Awaiting check-in.
									</Typography>
								) : (
									<Stack spacing={1} sx={{ mt: 0.5, ml: 2 }}>
										{box.applications.map((application) => (
											<ServerShorty
												key={application.id}
												server={application}
												current={application.id === currentApplicationId}
											/>
										))}
									</Stack>
								)}
							</Box>
						))}
					</Stack>
				</Box>
			))}
		</Stack>
	);
}
