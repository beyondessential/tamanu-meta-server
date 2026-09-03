import { Alert, LinearProgress, Stack, Typography } from "@mui/material";
import { Link as RouterLink, useParams } from "react-router-dom";
import { Navigate } from "react-router-dom";
import { useApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import { healthcheckPath, healthcheckSettingsPath } from "../types";

/// Lands a link that names a check by source and name alone on the entry it
/// means.
///
/// Check pages used to be addressed that way, so every bookmark and every
/// pasted link predating the namespace arrives here. Usually there is exactly
/// one entry with that name and the redirect is invisible; where two
/// application types report the name, the link is genuinely ambiguous and the
/// operator picks.
export default function CheckRedirect({ settings }: { settings: boolean }) {
	const { source, check } = useParams<{ source: string; check: string }>();
	usePageTitle(check ?? "Healthcheck");
	const list = useApi("healthchecks", "list");
	const path = settings ? healthcheckSettingsPath : healthcheckPath;

	if (list.status === "loading" || list.status === "idle") {
		return <LinearProgress />;
	}
	if (list.status === "error") {
		return <Alert severity="error">{list.error.message}</Alert>;
	}

	const matches = list.data.filter(
		(r) => r.source === source && r.check_name === check,
	);
	if (matches.length === 1) {
		const only = matches[0];
		return <Navigate to={path(only.source, only.namespace, only.check_name)} replace />;
	}
	if (matches.length === 0) {
		// Nothing in the catalog to resolve against, so the unqualified entry
		// is the only one this could have meant. Its page says so if it is
		// not there either.
		return (
			<Navigate
				to={path(source ?? "", { subject: null, application_type: null }, check ?? "")}
				replace
			/>
		);
	}
	return (
		<Stack spacing={1}>
			<Typography variant="h6" component="h2">
				Which {check}?
			</Typography>
			<Typography variant="body2" color="text.secondary">
				More than one application type reports this name to {source}, and they
				are different checks.
			</Typography>
			{matches.map((r) => (
				<Typography key={r.qualified_name} variant="body2">
					<RouterLink to={path(r.source, r.namespace, r.check_name)}>
						{r.qualified_name}
					</RouterLink>
				</Typography>
			))}
		</Stack>
	);
}
