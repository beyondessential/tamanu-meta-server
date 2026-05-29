import {
	Alert,
	Box,
	Button,
	Chip,
	Dialog,
	DialogActions,
	DialogContent,
	DialogTitle,
	IconButton,
	LinearProgress,
	MenuItem,
	Paper,
	Select,
	Stack,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	TextField,
	Typography,
} from "@mui/material";
import ArrowDownwardIcon from "@mui/icons-material/ArrowDownward";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import DeleteIcon from "@mui/icons-material/Delete";
import EditIcon from "@mui/icons-material/Edit";
import { useMemo, useState } from "react";
import { Link as RouterLink, useParams } from "react-router-dom";
import { ApiError, useApi, useApiAction } from "../api";
import SeverityChip from "../components/SeverityChip";
import TimeAgo from "../components/TimeAgo";
import { useIsAdmin } from "../hooks/useIsAdmin";
import { usePageTitle } from "../hooks/usePageTitle";
import {
	SEVERITIES,
	SEVERITY_INTENT,
	type HealthcheckSeverityData,
	type Severity,
} from "../types";

// ── Constrained JsonLogic shape ───────────────────────────────────────────
//
// The Rust deserialiser only accepts {if: [c1, s1, c2, s2, …]} ladders
// where each condition is {<op>: [{var: "<dotted>"}, <value>]}. The
// helpers below parse the raw JSON returned by the API into the typed
// shape this page edits, and serialize it back for /update_rules.

const RULE_OPS = ["==", "!=", "<", "<=", ">", ">=", "in_range"] as const;
type RuleOp = (typeof RULE_OPS)[number];
const OP_SYMBOL: Record<RuleOp, string> = {
	"==": "=",
	"!=": "≠",
	"<": "<",
	"<=": "≤",
	">": ">",
	">=": "≥",
	in_range: "∈",
};
const VAR_PATTERN = /^(check|status|tag)\.[A-Za-z0-9_]+$/;

interface Branch {
	varPath: string;
	op: RuleOp;
	value: unknown;
	severity: Severity;
}

function parseRules(raw: unknown): { branches: Branch[]; error: string | null } {
	if (raw == null) return { branches: [], error: null };
	if (typeof raw !== "object" || Array.isArray(raw)) {
		return { branches: [], error: "rules must be a JSON object" };
	}
	const obj = raw as Record<string, unknown>;
	const keys = Object.keys(obj);
	if (keys.length !== 1 || keys[0] !== "if") {
		return { branches: [], error: "rules must be {if: [...]}" };
	}
	const args = obj.if;
	if (!Array.isArray(args)) return { branches: [], error: "'if' value must be an array" };
	if (args.length % 2 !== 0) return { branches: [], error: "'if' args must be even-length" };
	const branches: Branch[] = [];
	for (let i = 0; i < args.length; i += 2) {
		const condRaw = args[i];
		const sevRaw = args[i + 1];
		const cond = parseCondition(condRaw);
		if (cond.error) return { branches: [], error: `branch ${i / 2}: ${cond.error}` };
		if (typeof sevRaw !== "string" || !(SEVERITIES as readonly string[]).includes(sevRaw)) {
			return {
				branches: [],
				error: `branch ${i / 2}: invalid severity '${String(sevRaw)}'`,
			};
		}
		branches.push({
			varPath: cond.varPath,
			op: cond.op,
			value: cond.value,
			severity: sevRaw as Severity,
		});
	}
	return { branches, error: null };
}

function parseCondition(raw: unknown): {
	varPath: string;
	op: RuleOp;
	value: unknown;
	error?: string;
} {
	const empty = { varPath: "", op: "==" as RuleOp, value: null };
	if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
		return { ...empty, error: "condition must be a JSON object" };
	}
	const obj = raw as Record<string, unknown>;
	const keys = Object.keys(obj);
	if (keys.length !== 1) {
		return { ...empty, error: "condition object must have exactly one key" };
	}
	const op = keys[0] as RuleOp;
	if (!(RULE_OPS as readonly string[]).includes(op)) {
		return { ...empty, error: `unknown op '${op}'` };
	}
	const args = obj[op];
	if (!Array.isArray(args) || args.length !== 2) {
		return { ...empty, error: `'${op}' must take exactly two args` };
	}
	const varObj = args[0];
	if (typeof varObj !== "object" || varObj === null || Array.isArray(varObj)) {
		return { ...empty, error: "first arg must be {var: '<dotted>'}" };
	}
	const varEntries = Object.entries(varObj as Record<string, unknown>);
	if (varEntries.length !== 1 || varEntries[0][0] !== "var" || typeof varEntries[0][1] !== "string") {
		return { ...empty, error: "first arg must be {var: '<dotted>'}" };
	}
	const varPath = varEntries[0][1] as string;
	return { varPath, op, value: args[1] };
}

function serializeRules(branches: Branch[]): unknown | null {
	if (branches.length === 0) return null;
	const args: unknown[] = [];
	for (const b of branches) {
		args.push({ [b.op]: [{ var: b.varPath }, b.value] });
		args.push(b.severity);
	}
	return { if: args };
}

// Best-effort parse of a free-form value input: try as number, else
// JSON, else fall back to the raw string. Lets operators write
// `95` and `"prod"` without thinking about quoting.
function parseValueInput(raw: string): unknown {
	const trimmed = raw.trim();
	if (trimmed === "") return "";
	if (trimmed === "true") return true;
	if (trimmed === "false") return false;
	const n = Number(trimmed);
	if (!Number.isNaN(n) && /^-?\d+(\.\d+)?$/.test(trimmed)) return n;
	try {
		return JSON.parse(trimmed);
	} catch {
		return trimmed;
	}
}

function valueToInputText(value: unknown): string {
	if (typeof value === "string") return value;
	if (value === null) return "null";
	return JSON.stringify(value);
}

// ── Page ──────────────────────────────────────────────────────────────────

export default function HealthcheckDetail() {
	const { checkName } = useParams<{ checkName: string }>();
	usePageTitle(checkName ?? "Healthcheck");
	const isAdmin = useIsAdmin() === true;
	const list = useApi("healthchecks", "list");

	const row: HealthcheckSeverityData | undefined =
		list.status === "ok" ? list.data.find((r) => r.check_name === checkName) : undefined;

	return (
		<Stack spacing={2}>
			<Box>
				<Typography variant="body2" color="text.secondary">
					<RouterLink to="/healthchecks">← All healthchecks</RouterLink>
				</Typography>
				<Typography variant="h6" component="h2" sx={{ fontFamily: "monospace" }}>
					{checkName}
				</Typography>
			</Box>

			{list.status === "loading" || list.status === "idle" ? (
				<LinearProgress />
			) : list.status === "error" ? (
				<Alert severity="error">{list.error.message}</Alert>
			) : row == null ? (
				<Alert severity="warning">
					No healthcheck named <code>{checkName}</code> in the catalog yet — it'll
					appear here once a server reports it.
				</Alert>
			) : (
				<>
					<RowMetadata row={row} />
					<BaseSeverityCard row={row} canEdit={isAdmin} onChanged={list.reload} />
					<NotesCard row={row} canEdit={isAdmin} onChanged={list.reload} />
					<RulesCard row={row} canEdit={isAdmin} onChanged={list.reload} />
				</>
			)}
		</Stack>
	);
}

function RowMetadata({ row }: { row: HealthcheckSeverityData }) {
	return (
		<Stack
			direction="row"
			spacing={2}
			sx={{ alignItems: "center", color: "text.secondary" }}
		>
			<Typography variant="caption">
				First seen <TimeAgo timestamp={row.first_seen} />
			</Typography>
			{row.pending_review ? (
				<Chip label="pending review" color="warning" size="small" />
			) : (
				<Typography variant="caption">
					Last reviewed{" "}
					{row.reviewed_at && <TimeAgo timestamp={row.reviewed_at} />}
					{row.reviewed_by && ` by ${row.reviewed_by}`}
				</Typography>
			)}
		</Stack>
	);
}

function BaseSeverityCard({
	row,
	canEdit,
	onChanged,
}: {
	row: HealthcheckSeverityData;
	canEdit: boolean;
	onChanged: () => void;
}) {
	const update = useApiAction("healthchecks", "update");
	const [severity, setSeverity] = useState<Severity>(row.severity);
	const save = async () => {
		try {
			await update.call({ check_name: row.check_name, severity, notes: row.notes });
			onChanged();
		} catch {
			setSeverity(row.severity);
		}
	};
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="subtitle1" gutterBottom>
				Base severity
			</Typography>
			<Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
				Used when this check fails and no rule below matches.
			</Typography>
			<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
				{canEdit ? (
					<Select
						size="small"
						value={severity}
						onChange={(e) => setSeverity(e.target.value as Severity)}
						disabled={update.pending}
						sx={{ minWidth: 320 }}
					>
						{SEVERITIES.map((s) => (
							<MenuItem key={s} value={s}>
								<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
									<SeverityChip severity={s} />
									<Typography variant="caption" color="text.secondary">
										{SEVERITY_INTENT[s]}
									</Typography>
								</Stack>
							</MenuItem>
						))}
					</Select>
				) : (
					<SeverityChip severity={row.severity} />
				)}
				{canEdit && (
					<Button size="small" variant="outlined" onClick={save} disabled={update.pending}>
						Save
					</Button>
				)}
			</Stack>
			{update.error && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{formatError(update.error)}
				</Alert>
			)}
		</Paper>
	);
}

function NotesCard({
	row,
	canEdit,
	onChanged,
}: {
	row: HealthcheckSeverityData;
	canEdit: boolean;
	onChanged: () => void;
}) {
	const update = useApiAction("healthchecks", "update");
	const [notes, setNotes] = useState<string>(row.notes ?? "");
	const save = async () => {
		try {
			await update.call({
				check_name: row.check_name,
				severity: row.severity,
				notes: notes || null,
			});
			onChanged();
		} catch {
			// Leave the textarea where it is so the user can retry.
		}
	};
	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Typography variant="subtitle1" gutterBottom>
				Notes
			</Typography>
			<TextField
				multiline
				minRows={2}
				fullWidth
				size="small"
				placeholder="Operator commentary on this check (visible only on this page)"
				value={notes}
				onChange={(e) => setNotes(e.target.value)}
				disabled={!canEdit || update.pending}
			/>
			{canEdit && (
				<Box sx={{ mt: 1 }}>
					<Button size="small" variant="outlined" onClick={save} disabled={update.pending}>
						Save notes
					</Button>
				</Box>
			)}
			{update.error && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{formatError(update.error)}
				</Alert>
			)}
		</Paper>
	);
}

// ── Rules editor ──────────────────────────────────────────────────────────

function RulesCard({
	row,
	canEdit,
	onChanged,
}: {
	row: HealthcheckSeverityData;
	canEdit: boolean;
	onChanged: () => void;
}) {
	const parsed = useMemo(() => parseRules(row.rules), [row.rules]);
	const [branches, setBranches] = useState<Branch[]>(parsed.branches);
	const [dialog, setDialog] = useState<{ index: number | null } | null>(null);
	const update = useApiAction("healthchecks", "update_rules");

	const dirty = useMemo(
		() => JSON.stringify(branches) !== JSON.stringify(parsed.branches),
		[branches, parsed.branches],
	);

	const save = async () => {
		try {
			await update.call({ check_name: row.check_name, rules: serializeRules(branches) });
			onChanged();
		} catch {
			// keep local state for retry
		}
	};
	const deleteAll = async () => {
		setBranches([]);
		try {
			await update.call({ check_name: row.check_name, rules: null });
			onChanged();
		} catch {
			setBranches(parsed.branches);
		}
	};

	return (
		<Paper variant="outlined" sx={{ p: 2 }}>
			<Stack direction="row" sx={{ alignItems: "center", mb: 1 }}>
				<Typography variant="subtitle1" sx={{ flex: 1 }}>
					Conditional rules
				</Typography>
				{canEdit && (
					<Button
						size="small"
						variant="contained"
						onClick={() => setDialog({ index: null })}
						disabled={update.pending}
					>
						Add rule
					</Button>
				)}
			</Stack>
			<Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
				Evaluated top-to-bottom on every failing push for this check. First matching
				branch's severity wins; if none match, the base severity above is used.
			</Typography>
			{parsed.error && (
				<Alert severity="warning" sx={{ mb: 1 }}>
					Stored rules are malformed ({parsed.error}) — ingestion falls back to the
					base severity until you save valid rules from this page.
				</Alert>
			)}
			{branches.length === 0 ? (
				<Alert severity="info">No rules. The base severity is used for every push.</Alert>
			) : (
				<TableContainer>
					<Table size="small">
						<TableHead>
							<TableRow>
								<TableCell width={40}>#</TableCell>
								<TableCell>Condition</TableCell>
								<TableCell>Severity</TableCell>
								{canEdit && <TableCell width={150} />}
							</TableRow>
						</TableHead>
						<TableBody>
							{branches.map((b, idx) => (
								<TableRow key={idx} hover>
									<TableCell>{idx}</TableCell>
									<TableCell sx={{ fontFamily: "monospace", fontSize: "0.85em" }}>
										{b.varPath} {OP_SYMBOL[b.op]} {valueToInputText(b.value)}
									</TableCell>
									<TableCell>
										<SeverityChip severity={b.severity} />
									</TableCell>
									{canEdit && (
										<TableCell>
											<Stack direction="row" spacing={0.5}>
												<IconButton
													size="small"
													disabled={idx === 0}
													onClick={() =>
														setBranches((bs) => swap(bs, idx, idx - 1))
													}
												>
													<ArrowUpwardIcon fontSize="small" />
												</IconButton>
												<IconButton
													size="small"
													disabled={idx === branches.length - 1}
													onClick={() =>
														setBranches((bs) => swap(bs, idx, idx + 1))
													}
												>
													<ArrowDownwardIcon fontSize="small" />
												</IconButton>
												<IconButton
													size="small"
													onClick={() => setDialog({ index: idx })}
												>
													<EditIcon fontSize="small" />
												</IconButton>
												<IconButton
													size="small"
													onClick={() =>
														setBranches((bs) =>
															bs.filter((_, i) => i !== idx),
														)
													}
												>
													<DeleteIcon fontSize="small" />
												</IconButton>
											</Stack>
										</TableCell>
									)}
								</TableRow>
							))}
						</TableBody>
					</Table>
				</TableContainer>
			)}
			{canEdit && (
				<Stack direction="row" spacing={1} sx={{ mt: 2 }}>
					<Button
						size="small"
						variant="contained"
						onClick={save}
						disabled={!dirty || update.pending}
					>
						Save rules
					</Button>
					{branches.length > 0 && (
						<Button
							size="small"
							color="error"
							onClick={deleteAll}
							disabled={update.pending}
						>
							Delete all rules
						</Button>
					)}
				</Stack>
			)}
			{update.error && (
				<Alert severity="error" sx={{ mt: 1 }}>
					{formatError(update.error)}
				</Alert>
			)}
			{dialog && (
				<BranchDialog
					initial={dialog.index != null ? branches[dialog.index] : null}
					onCancel={() => setDialog(null)}
					onSave={(b) => {
						setBranches((bs) => {
							if (dialog.index == null) return [...bs, b];
							const next = [...bs];
							next[dialog.index] = b;
							return next;
						});
						setDialog(null);
					}}
				/>
			)}
		</Paper>
	);
}

function swap<T>(arr: T[], i: number, j: number): T[] {
	const next = [...arr];
	[next[i], next[j]] = [next[j], next[i]];
	return next;
}

function BranchDialog({
	initial,
	onCancel,
	onSave,
}: {
	initial: Branch | null;
	onCancel: () => void;
	onSave: (b: Branch) => void;
}) {
	const [varPath, setVarPath] = useState(initial?.varPath ?? "status.");
	const [op, setOp] = useState<RuleOp>(initial?.op ?? "==");
	const [valueText, setValueText] = useState(
		initial ? valueToInputText(initial.value) : "",
	);
	const [severity, setSeverity] = useState<Severity>(initial?.severity ?? "error");
	const varValid = VAR_PATTERN.test(varPath);

	const submit = () => {
		if (!varValid) return;
		onSave({
			varPath,
			op,
			value: parseValueInput(valueText),
			severity,
		});
	};

	return (
		<Dialog open onClose={onCancel} fullWidth maxWidth="sm">
			<DialogTitle>{initial ? "Edit rule" : "Add rule"}</DialogTitle>
			<DialogContent>
				<Stack spacing={2} sx={{ mt: 1 }}>
					<TextField
						label="Variable"
						helperText={
							varValid
								? "Path like status.bestoolVersion, check.used_pct, tag.environment"
								: "Must match check.<field>, status.<field>, or tag.<field>"
						}
						error={!varValid}
						size="small"
						value={varPath}
						onChange={(e) => setVarPath(e.target.value)}
					/>
					<Stack direction="row" spacing={1}>
						<Select
							size="small"
							value={op}
							onChange={(e) => setOp(e.target.value as RuleOp)}
							sx={{ minWidth: 100 }}
						>
							{RULE_OPS.map((o) => (
								<MenuItem key={o} value={o}>
									{OP_SYMBOL[o]} ({o})
								</MenuItem>
							))}
						</Select>
						<TextField
							label="Value"
							size="small"
							fullWidth
							helperText={
								op === "in_range"
									? "semver range, e.g. >=2.4.0 <2.5.4 or ^2.6"
									: "number, string, or JSON literal"
							}
							value={valueText}
							onChange={(e) => setValueText(e.target.value)}
						/>
					</Stack>
					<Select
						size="small"
						value={severity}
						onChange={(e) => setSeverity(e.target.value as Severity)}
						sx={{ minWidth: 320 }}
					>
						{SEVERITIES.map((s) => (
							<MenuItem key={s} value={s}>
								<Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
									<SeverityChip severity={s} />
									<Typography variant="caption" color="text.secondary">
										{SEVERITY_INTENT[s]}
									</Typography>
								</Stack>
							</MenuItem>
						))}
					</Select>
				</Stack>
			</DialogContent>
			<DialogActions>
				<Button onClick={onCancel}>Cancel</Button>
				<Button variant="contained" onClick={submit} disabled={!varValid}>
					{initial ? "Save" : "Add"}
				</Button>
			</DialogActions>
		</Dialog>
	);
}

function formatError(err: unknown): string {
	if (err instanceof ApiError) {
		const detail = err.detail as { title?: string } | null;
		if (detail?.title) return detail.title;
		return err.message;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}
