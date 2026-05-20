import {
	Accordion,
	AccordionDetails,
	AccordionSummary,
	Alert,
	Box,
	Chip,
	LinearProgress,
	Link as MuiLink,
	Stack,
	Tooltip,
	Typography,
} from "@mui/material";
import ErrorOutlineIcon from "@mui/icons-material/ErrorOutlined";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import { Link as RouterLink } from "react-router-dom";
import VersionStatusChip from "../components/VersionStatusChip";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import type { MinorVersionGroup } from "../types";

export default function Versions() {
	usePageTitle("Versions");
	const result = useApi("versions", "get_grouped_versions");

	return (
		<Stack spacing={3}>
			<Typography variant="h4" component="h1">
				Versions
			</Typography>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : result.data.length === 0 ? (
				<Alert severity="info">No versions found</Alert>
			) : (
				<Box>
					{result.data.map((group) => (
						<MinorGroup key={`${group.major}.${group.minor}`} group={group} />
					))}
				</Box>
			)}
		</Stack>
	);
}

function MinorGroup({ group }: { group: MinorVersionGroup }) {
	return (
		<Accordion
			disableGutters
			elevation={0}
			sx={{
				my: 1,
				"&:before": { display: "none" },
				borderRadius: 1,
				border: 1,
				borderColor: "divider",
				boxShadow: "none",
				"&.Mui-expanded": { boxShadow: "none" },
			}}
		>
			<AccordionSummary
				expandIcon={<ExpandMoreIcon />}
				sx={{
					transition: "background-color 150ms",
					"&:hover": { bgcolor: "action.hover" },
				}}
			>
				<Stack
					direction="row"
					spacing={2}
					sx={{ width: "100%", alignItems: "center" }}
				>
					<Typography
						variant="body1"
						sx={{ fontFamily: "monospace", fontWeight: 600 }}
					>
						{group.major}.{group.minor}.{group.latest_patch}
					</Typography>
					<Typography variant="body2" color="text.secondary" sx={{ ml: "auto !important" }}>
						{group.count} version{group.count === 1 ? "" : "s"}
					</Typography>
					<Typography variant="body2" color="text.secondary">
						{formatDate(group.first_created_at)}
					</Typography>
				</Stack>
			</AccordionSummary>
			<AccordionDetails>
				<Stack spacing={1}>
					{group.versions.map((v) => (
						<MuiLink
							key={`${v.major}.${v.minor}.${v.patch}`}
							component={RouterLink}
							to={`/versions/${v.major}.${v.minor}.${v.patch}`}
							underline="none"
							color="inherit"
						>
							<Box
								sx={(theme) => ({
									p: 1.5,
									borderRadius: 1,
									border: `1px solid ${theme.palette.divider}`,
									bgcolor:
										v.status === "draft"
											? theme.palette.warning.light + "22"
											: v.status === "yanked"
												? theme.palette.error.light + "22"
												: undefined,
									transition: "background-color 150ms",
									"&:hover": { bgcolor: theme.palette.action.hover },
								})}
							>
								<Stack
									direction="row"
									spacing={2}
									sx={{ alignItems: "center" }}
								>
									<Typography sx={{ fontFamily: "monospace" }}>
										{v.major}.{v.minor}.{v.patch}
									</Typography>
									{v.status !== "published" && (
										<VersionStatusChip status={v.status} />
									)}
									{!v.ready && (
										<Tooltip title="This version has unresolved known issues">
											<Chip
												size="small"
												color="warning"
												variant="outlined"
												icon={<ErrorOutlineIcon />}
												label="Known issues"
											/>
										</Tooltip>
									)}
									<Typography
										variant="body2"
										color="text.secondary"
										sx={{ ml: "auto !important" }}
									>
										{formatDate(v.created_at)}
									</Typography>
								</Stack>
							</Box>
						</MuiLink>
					))}
				</Stack>
			</AccordionDetails>
		</Accordion>
	);
}

function formatDate(iso: string): string {
	try {
		return iso.slice(0, 10);
	} catch {
		return iso;
	}
}
