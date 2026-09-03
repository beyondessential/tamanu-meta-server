import { LinearProgress, Alert } from "@mui/material";
import { Navigate, useParams } from "react-router-dom";
import { useApi } from "../api";

/// Editing an application sends the operator to its machine's form.
///
/// There is one form per machine, holding the box's section and one section per
/// application on it, so a machine fact has one place to be edited and a shared
/// box is edited where everything sharing it is visible. An application's own
/// fields are a section of that form rather than a form of their own.
///
/// This route stays so the links that point at it keep working; it resolves the
/// application's machine and hands over.
/// spec: FLT#groups
export default function ServerEdit() {
	const { id = "" } = useParams<{ id: string }>();
	const detail = useApi("fleet/applications", "get_detail", { server_id: id }, [id]);

	if (detail.status === "loading" || detail.status === "idle") {
		return <LinearProgress />;
	}
	if (detail.status === "error") {
		return <Alert severity="error">{detail.error.message}</Alert>;
	}
	return <Navigate to={`/fleet/machines/${detail.data.server.machine_id}/edit`} replace />;
}
