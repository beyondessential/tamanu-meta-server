import {
	Alert as MuiAlert,
	Box,
	Button,
	MenuItem,
	Stack,
	TextField,
} from "@mui/material";
import { useState } from "react";
import { useApiAction } from "../api";
import { SEVERITIES, type Severity } from "../types";

export default function ManualEventForm({
	serverId,
	onSubmitted,
}: {
	serverId: string;
	onSubmitted?: () => void;
}) {
	const [severity, setSeverity] = useState<Severity>("error");
	const [message, setMessage] = useState("");
	const [description, setDescription] = useState("");

	const action = useApiAction("issues", "submit_manual_event");

	const submit = async () => {
		try {
			await action.call({
				serverId,
				// Each manual submission is its own issue. Operators that need to
				// add context to an existing issue use the notes panel instead.
				ref: crypto.randomUUID(),
				severity,
				description: description.trim() === "" ? null : description.trim(),
				message,
			});
			setMessage("");
			setDescription("");
			onSubmitted?.();
		} catch {
			/* surfaced via action.error */
		}
	};

	const valid = message.trim() !== "";

	return (
		<Stack spacing={2} sx={{ pt: 1 }}>
			<TextField
				select
				label="Severity"
				size="small"
				value={severity}
				onChange={(e) => setSeverity(e.target.value as Severity)}
				sx={{ minWidth: 140, alignSelf: "flex-start" }}
			>
				{SEVERITIES.map((s) => (
					<MenuItem key={s} value={s}>
						{s}
					</MenuItem>
				))}
			</TextField>
			<TextField
				label="Description (short)"
				size="small"
				value={description}
				onChange={(e) => setDescription(e.target.value)}
			/>
			<TextField
				label="Message"
				size="small"
				value={message}
				onChange={(e) => setMessage(e.target.value)}
				required
				multiline
				minRows={2}
			/>
			{action.error && (
				<MuiAlert severity="error">{action.error.message}</MuiAlert>
			)}
			<Box>
				<Button
					variant="contained"
					onClick={submit}
					disabled={!valid || action.pending}
				>
					{action.pending ? "Submitting…" : "Submit"}
				</Button>
			</Box>
		</Stack>
	);
}
