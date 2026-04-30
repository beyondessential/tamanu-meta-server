import { Box } from "@mui/material";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export default function Markdown({ children }: { children: string }) {
	return (
		<Box
			sx={{
				"& h1, & h2, & h3, & h4, & h5, & h6": {
					mt: 2,
					mb: 1,
				},
				"& p": { my: 1 },
				"& ul, & ol": { pl: 3, my: 1 },
				"& code": {
					fontFamily: "monospace",
					px: 0.5,
					borderRadius: 0.5,
					bgcolor: "action.hover",
				},
				"& pre code": { bgcolor: "transparent", p: 0 },
				"& pre": {
					p: 1.5,
					borderRadius: 1,
					bgcolor: "action.hover",
					overflow: "auto",
				},
				"& table": {
					borderCollapse: "collapse",
					my: 1,
				},
				"& th, & td": {
					border: 1,
					borderColor: "divider",
					p: 0.5,
				},
				"& a": {
					color: "primary.main",
					textDecoration: "underline",
				},
				"& blockquote": {
					borderLeft: 3,
					borderColor: "divider",
					ml: 0,
					pl: 2,
					color: "text.secondary",
				},
			}}
		>
			<ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
		</Box>
	);
}
