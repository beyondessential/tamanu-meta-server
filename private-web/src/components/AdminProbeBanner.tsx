import { Alert, AlertTitle, Box, Button } from "@mui/material";
import { useAdminStatus } from "../hooks/useIsAdmin";

/// Shown while the admin probe has never answered and its last attempt failed.
/// Without it the failure is invisible: admin-only controls are simply absent,
/// and an operator who *is* an admin has no way to tell "you can't do this"
/// from "canopy couldn't work out whether you can".
///
/// Only the unresolved case is worth a banner. Once the probe has answered, the
/// provider keeps that answer sticky, so a later blip changes nothing on screen
/// — and if the answer was stale, the action itself fails loudly with a 403.
export default function AdminProbeBanner() {
	const { resolved, error, reload } = useAdminStatus();
	if (resolved || !error) return null;

	return (
		<Box sx={{ px: 3, pt: 2 }}>
			<Alert
				severity="warning"
				action={
					<Button color="inherit" size="small" onClick={reload}>
						Retry
					</Button>
				}
			>
				<AlertTitle sx={{ mb: 0 }}>
					Couldn't check your admin status
				</AlertTitle>
				Admin-only controls are hidden until this succeeds. Retrying
				automatically. ({error.message})
			</Alert>
		</Box>
	);
}
