import { Box, IconButton, Tooltip, Typography } from "@mui/material";
import CodeIcon from "@mui/icons-material/Code";
import NotesIcon from "@mui/icons-material/Notes";
import { useState } from "react";
import Markdown from "./Markdown";

/** Renders a message body that may contain markdown or a limited subset of
 * HTML. The toggle in the top-right switches between the rendered view and
 * the raw text (monospace, pre-wrap). HTML is sanitised by `Markdown`. */
export default function MessageView({ message }: { message: string }) {
	const [raw, setRaw] = useState(false);
	return (
		<Box sx={{ position: "relative" }}>
			<Box sx={{ position: "absolute", top: 0, right: 0 }}>
				<Tooltip title={raw ? "Show rendered" : "Show raw text"}>
					<IconButton
						aria-label={raw ? "Show rendered" : "Show raw text"}
						size="small"
						onClick={() => setRaw((v) => !v)}
					>
						{raw ? (
							<NotesIcon fontSize="small" />
						) : (
							<CodeIcon fontSize="small" />
						)}
					</IconButton>
				</Tooltip>
			</Box>
			{raw ? (
				<Typography
					variant="body2"
					component="pre"
					sx={{
						m: 0,
						pr: 4,
						whiteSpace: "pre-wrap",
						fontFamily: "monospace",
						fontSize: "0.85em",
					}}
				>
					{message}
				</Typography>
			) : (
				<Box sx={{ pr: 4, "& > :first-child": { mt: 0 }, "& > :last-child": { mb: 0 } }}>
					<Markdown preserveNewlines>{message}</Markdown>
				</Box>
			)}
		</Box>
	);
}
