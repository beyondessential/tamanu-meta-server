import { Alert, Box, IconButton, Stack, TextField, Tooltip } from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import DeleteIcon from "@mui/icons-material/DeleteOutlined";
import { useMemo, useState } from "react";

export type TagMap = Record<string, string>;

interface Row {
	key: string;
	value: string;
	/** Stable React key — survives reordering and lets a half-typed key keep
	 * focus without the row identity flipping under it. */
	rowId: number;
}

/**
 * The component holds its own list of rows so a freshly-added row with an
 * empty key can sit on screen while the operator types one. The parent only
 * ever sees a deduplicated `TagMap` (empty keys are dropped on emit), which
 * means a blank row round-tripping through the parent would vanish — hence
 * the local mirror.
 *
 * The initial state seeds from `value` on mount; later parent updates are
 * not re-read. Parents that need to reset the editor (e.g. cancel) should
 * key on the entity id with React's `key` prop to force a remount.
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
	const [rows, setRows] = useState<Row[]>(() => initRows(value));

	const update = (next: Row[]) => {
		setRows(next);
		const out: TagMap = {};
		for (const r of next) {
			const k = r.key.trim();
			if (k === "") continue;
			// Last write wins on duplicates — surfaced as a warning below.
			out[k] = r.value;
		}
		onChange(out);
	};

	const setKey = (i: number, key: string) => {
		const next = rows.slice();
		next[i] = { ...next[i]!, key };
		update(next);
	};
	const setValue = (i: number, val: string) => {
		const next = rows.slice();
		next[i] = { ...next[i]!, value: val };
		update(next);
	};
	const removeRow = (i: number) => {
		const next = rows.slice();
		next.splice(i, 1);
		update(next);
	};
	const addRow = () => {
		update([...rows, { key: "", value: "", rowId: nextRowId() }]);
	};

	const dupKeys = useMemo(() => {
		const seen = new Set<string>();
		const dups = new Set<string>();
		for (const r of rows) {
			const t = r.key.trim();
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
			{rows.map((row, i) => (
				<Stack
					key={row.rowId}
					direction={{ xs: "column", sm: "row" }}
					spacing={1}
					sx={{ alignItems: { sm: "flex-start" } }}
				>
					<TextField
						label="Key"
						value={row.key}
						onChange={(e) => setKey(i, e.target.value)}
						disabled={disabled}
						error={dupKeys.has(row.key.trim())}
						helperText={dupKeys.has(row.key.trim()) ? "duplicate key" : undefined}
						sx={{ flex: 1 }}
					/>
					<TextField
						label="Value"
						value={row.value}
						onChange={(e) => setValue(i, e.target.value)}
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

let rowIdCounter = 0;
function nextRowId(): number {
	rowIdCounter += 1;
	return rowIdCounter;
}

function initRows(map: TagMap): Row[] {
	return Object.entries(map)
		.sort(([a], [b]) => a.localeCompare(b))
		.map(([key, value]) => ({ key, value, rowId: nextRowId() }));
}
