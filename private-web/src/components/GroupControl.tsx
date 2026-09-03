import { Autocomplete, Stack, TextField, Typography } from "@mui/material";
import { useEffect, useMemo, useState } from "react";
import { callApi, useApi } from "../api";
import type { ServerGroup } from "../types";

/// Pick the group a machine belongs to: typeahead over the group search, with
/// the full list as the default options so the current selection can be named.
///
/// A group is the machine's, and the applications on it take it — there is no
/// separate group control on an application (see FLT, "Groups").
/// spec: FLT#groups
export default function GroupControl({
	currentGroupId,
	onChange,
	disabled,
	required = false,
}: {
	currentGroupId: string | null;
	onChange: (groupId: string | null) => void;
	disabled: boolean;
	required?: boolean;
}) {
	const [query, setQuery] = useState("");
	const [results, setResults] = useState<ServerGroup[]>([]);
	const [loading, setLoading] = useState(false);

	const allGroups = useApi("fleet/groups", "list", {}, []);

	useEffect(() => {
		if (!query) {
			setResults([]);
			return;
		}
		let cancelled = false;
		setLoading(true);
		(async () => {
			try {
				const found = await callApi("fleet/groups", "search", { query });
				if (!cancelled) setResults(found);
			} catch {
				if (!cancelled) setResults([]);
			} finally {
				if (!cancelled) setLoading(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [query]);

	const currentValue = useMemo<ServerGroup | null>(() => {
		if (!currentGroupId) return null;
		if (allGroups.status === "ok") {
			return allGroups.data.find((g) => g.id === currentGroupId) ?? null;
		}
		return null;
	}, [currentGroupId, allGroups]);

	const options = useMemo<ServerGroup[]>(() => {
		if (query) return results;
		return allGroups.status === "ok" ? allGroups.data : [];
	}, [query, results, allGroups]);

	return (
		<Autocomplete<ServerGroup, false, false, false>
			disabled={disabled}
			options={options}
			value={currentValue}
			onChange={(_, v) => onChange(v?.id ?? null)}
			onInputChange={(_, v) => setQuery(v)}
			loading={loading}
			getOptionLabel={(g) => g.name}
			isOptionEqualToValue={(a, b) => a.id === b.id}
			filterOptions={(x) => x}
			renderInput={(params) => {
				const missing = required && !currentValue;
				return (
					<TextField
						{...params}
						label="Group"
						required={required}
						error={missing}
						placeholder="Search by name, or pick from the list"
						helperText={
							missing
								? "Required — every machine belongs to a group."
								: "The group this machine belongs to."
						}
					/>
				);
			}}
			renderOption={(props, group) => (
				<li {...props} key={group.id}>
					<Stack>
						<Typography variant="body2">{group.name}</Typography>
						{group.notes && (
							<Typography
								variant="caption"
								color="text.secondary"
								sx={{
									overflow: "hidden",
									textOverflow: "ellipsis",
									whiteSpace: "nowrap",
									maxWidth: "60ch",
								}}
							>
								{group.notes.split("\n")[0]}
							</Typography>
						)}
					</Stack>
				</li>
			)}
		/>
	);
}
