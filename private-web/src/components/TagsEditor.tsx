import { Alert, Box, IconButton, Stack, TextField, Tooltip } from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import DeleteIcon from "@mui/icons-material/DeleteOutline";
import { useMemo } from "react";

export type TagMap = Record<string, string>;

/**
 * Stable row identity is keyed by index — the parent passes the full map back
 * each onChange and we render from the entries-sorted-by-key view. To allow
 * editing the key without losing focus, we hold the row list locally as
 * `(key, value)` pairs (preserving entry order) and only materialise the
 * deduplicated TagMap on change.
 */
export default function TagsEditor({
	value,
	onChange,
	disabled,
}: {
	value: TagMap;
	onChange: (next: TagMap) => void;
	disabled?: boolean;
}) {
	const rows: Array<[string, string]> = useMemo(
		() => Object.entries(value).sort(([a], [b]) => a.localeCompare(b)),
		[value],
	);

	const updateRow = (index: number, key: string, val: string) => {
		const next = rows.slice();
		next[index] = [key, val];
		emit(next);
	};

	const removeRow = (index: number) => {
		const next = rows.slice();
		next.splice(index, 1);
		emit(next);
	};

	const addRow = () => {
		emit([...rows, ["", ""]]);
	};

	const emit = (entries: Array<[string, string]>) => {
		const out: TagMap = {};
		for (const [k, v] of entries) {
			const trimmed = k.trim();
			if (trimmed === "") continue;
			out[trimmed] = v;
		}
		onChange(out);
	};

	const dupKeys = useMemo(() => {
		const seen = new Set<string>();
		const dups = new Set<string>();
		for (const [k] of rows) {
			const t = k.trim();
			if (t === "") continue;
			if (seen.has(t)) dups.add(t);
			seen.add(t);
		}
		return dups;
	}, [rows]);

	return (
		<Stack spacing={1}>
			{rows.length === 0 && (
				<Box sx={{ color: "text.secondary", fontSize: 14 }}>No tags.</Box>
			)}
			{rows.map(([k, v], i) => (
				<Stack
					key={i}
					direction={{ xs: "column", sm: "row" }}
					spacing={1}
					sx={{ alignItems: { sm: "flex-start" } }}
				>
					<TextField
						label="Key"
						value={k}
						onChange={(e) => updateRow(i, e.target.value, v)}
						disabled={disabled}
						error={dupKeys.has(k.trim())}
						helperText={dupKeys.has(k.trim()) ? "duplicate key" : undefined}
						sx={{ flex: 1 }}
					/>
					<TextField
						label="Value"
						value={v}
						onChange={(e) => updateRow(i, k, e.target.value)}
						disabled={disabled}
						sx={{ flex: 2 }}
					/>
					<Tooltip title="Remove">
						<span>
							<IconButton
								aria-label="remove tag"
								onClick={() => removeRow(i)}
								disabled={disabled}
							>
								<DeleteIcon />
							</IconButton>
						</span>
					</Tooltip>
				</Stack>
			))}
			{dupKeys.size > 0 && (
				<Alert severity="warning">
					Duplicate keys are merged on save — only the last value wins. Rename
					or remove duplicates to be explicit.
				</Alert>
			)}
			<Box>
				<IconButton
					onClick={addRow}
					disabled={disabled}
					size="small"
					sx={{ border: 1, borderColor: "divider", borderRadius: 1 }}
					aria-label="add tag"
				>
					<AddIcon />
				</IconButton>
			</Box>
		</Stack>
	);
}
