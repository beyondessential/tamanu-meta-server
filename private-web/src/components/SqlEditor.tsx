import { sql, PostgreSQL } from "@codemirror/lang-sql";
import { useTheme } from "@mui/material";
import CodeMirror from "@uiw/react-codemirror";

interface SqlEditorProps {
	value: string;
	onChange: (next: string) => void;
	readOnly?: boolean;
	minHeight?: string;
	placeholder?: string;
}

export default function SqlEditor({
	value,
	onChange,
	readOnly = false,
	minHeight = "10em",
	placeholder,
}: SqlEditorProps) {
	const theme = useTheme();
	return (
		<CodeMirror
			value={value}
			height="auto"
			minHeight={minHeight}
			readOnly={readOnly}
			placeholder={placeholder}
			theme={theme.palette.mode === "dark" ? "dark" : "light"}
			extensions={[sql({ dialect: PostgreSQL, upperCaseKeywords: true })]}
			onChange={onChange}
			basicSetup={{
				lineNumbers: true,
				highlightActiveLine: !readOnly,
				highlightActiveLineGutter: !readOnly,
				autocompletion: !readOnly,
			}}
		/>
	);
}
