import {
	Alert,
	Box,
	LinearProgress,
	Paper,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	Typography,
} from "@mui/material";
import { useParams } from "react-router-dom";
import Markdown from "../components/Markdown";
import VersionStatusChip from "../components/VersionStatusChip";
import { useApi } from "../api";
import type {
	ArtifactData,
	RelatedVersionData,
	VersionDetail as VersionDetailData,
} from "../types";

export default function VersionDetail() {
	const { version = "" } = useParams<{ version: string }>();
	const detail = useApi<VersionDetailData>(
		"versions",
		"get_version_detail",
		{ version },
		[version],
	);

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <Alert severity="error">{detail.error.message}</Alert>;
	}

	const v = detail.data;
	const versionStr = `${v.major}.${v.minor}.${v.patch}`;

	return (
		<Stack spacing={3}>
			<Stack
				direction="row"
				spacing={2}
				sx={{ alignItems: "center", justifyContent: "space-between" }}
			>
				<Typography variant="h4" component="h1" sx={{ fontFamily: "monospace" }}>
					{versionStr}
				</Typography>
				<VersionStatusChip status={v.status} />
			</Stack>

			<VersionInfo detail={v} />

			<ArtifactsSection versionId={v.id} />

			<Box>
				<Typography variant="h5" component="h2" gutterBottom>
					Changelog
				</Typography>
				<Paper variant="outlined" sx={{ p: 2 }}>
					{v.changelog ? (
						<Markdown>{v.changelog}</Markdown>
					) : (
						<Typography variant="body2" color="text.secondary">
							No changelog
						</Typography>
					)}
				</Paper>
			</Box>

			{v.related_versions.length > 0 && (
				<RelatedVersionsSection related={v.related_versions} />
			)}
		</Stack>
	);
}

function VersionInfo({ detail }: { detail: VersionDetailData }) {
	const items = [
		{ label: "Created", value: formatDate(detail.created_at) },
		{ label: "Last updated", value: formatDate(detail.updated_at) },
		detail.min_chrome_version != null && {
			label: "Chrome support",
			value: `${detail.min_chrome_version} or later`,
		},
	].filter(Boolean) as Array<{ label: string; value: string }>;

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack
				direction="row"
				spacing={4}
				useFlexGap
				sx={{ flexWrap: "wrap" }}
			>
				{items.map(({ label, value }) => (
					<Stack key={label} spacing={0.25}>
						<Typography variant="caption" color="text.secondary">
							{label}
						</Typography>
						<Typography variant="body2">{value}</Typography>
					</Stack>
				))}
			</Stack>
		</Paper>
	);
}

function ArtifactsSection({ versionId }: { versionId: string }) {
	const result = useApi<ArtifactData[]>(
		"versions",
		"get_artifacts_by_version_id",
		{ version_id: versionId },
		[versionId],
	);

	return (
		<Box>
			<Typography variant="h5" component="h2" gutterBottom>
				Artifacts
			</Typography>
			{result.status === "loading" || result.status === "idle" ? (
				<LinearProgress />
			) : result.status === "error" ? (
				<Alert severity="error">{result.error.message}</Alert>
			) : result.data.length === 0 ? (
				<Alert severity="info">No artifacts for this version.</Alert>
			) : (
				<TableContainer component={Paper} variant="outlined">
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell>Type</TableCell>
								<TableCell>Platform</TableCell>
								<TableCell>Download URL</TableCell>
								<TableCell>Range</TableCell>
							</TableRow>
						</TableHead>
						<TableBody>
							{result.data.map((a) => (
								<TableRow key={a.id}>
									<TableCell sx={{ fontFamily: "monospace" }}>
										{a.artifact_type}
										{a.has_range_override && (
											<Typography
												variant="caption"
												color="warning.main"
												sx={{ display: "block" }}
											>
												Overrides other artifact
											</Typography>
										)}
									</TableCell>
									<TableCell sx={{ fontFamily: "monospace" }}>
										{a.platform}
									</TableCell>
									<TableCell sx={{ wordBreak: "break-all" }}>
										{a.download_url.startsWith("https://") ? (
											<a
												href={a.download_url}
												target="_blank"
												rel="noopener noreferrer"
											>
												{a.download_url}
											</a>
										) : (
											a.download_url
										)}
									</TableCell>
									<TableCell sx={{ fontFamily: "monospace" }}>
										{a.version_range_pattern ?? "exact"}
										{a.version_range_pattern && !a.is_used_in_public_api && (
											<Typography
												variant="caption"
												color="error"
												sx={{ display: "block" }}
											>
												Hidden
											</Typography>
										)}
									</TableCell>
								</TableRow>
							))}
						</TableBody>
					</Table>
				</TableContainer>
			)}
		</Box>
	);
}

function RelatedVersionsSection({
	related,
}: {
	related: RelatedVersionData[];
}) {
	return (
		<Box>
			<Typography variant="h5" component="h2" gutterBottom>
				Related versions
			</Typography>
			<Stack spacing={2}>
				{related.map((r) => (
					<Box key={`${r.major}.${r.minor}.${r.patch}`}>
						<Typography
							variant="h6"
							component="h3"
							sx={{ fontFamily: "monospace" }}
						>
							{r.major}.{r.minor}.{r.patch}
						</Typography>
						<Paper variant="outlined" sx={{ p: 2 }}>
							{r.changelog ? (
								<Markdown>{r.changelog}</Markdown>
							) : (
								<Typography variant="body2" color="text.secondary">
									No changelog
								</Typography>
							)}
						</Paper>
					</Box>
				))}
			</Stack>
		</Box>
	);
}

function formatDate(iso: string): string {
	return iso.slice(0, 10);
}
