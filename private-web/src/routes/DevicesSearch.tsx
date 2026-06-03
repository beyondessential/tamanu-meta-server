import {
	Alert,
	LinearProgress,
	Stack,
	TextField,
	Typography,
} from "@mui/material";
import { useEffect, useState } from "react";
import DeviceShorty from "../components/DeviceShorty";
import { callApi } from "../api";
import { usePageTitle } from "../hooks/usePageTitle";
import type { DeviceInfo } from "../types";

export default function DevicesSearch() {
	usePageTitle("Search devices");
	const [query, setQuery] = useState("");
	const [results, setResults] = useState<DeviceInfo[] | null>(null);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		if (!query.trim()) {
			setResults(null);
			setError(null);
			return;
		}
		let cancelled = false;
		setLoading(true);
		setError(null);
		(async () => {
			try {
				const found = await callApi(
					"devices",
					"search",
					{ query: query.trim() },
				);
				if (!cancelled) setResults(found);
			} catch (err) {
				if (!cancelled)
					setError(err instanceof Error ? err.message : String(err));
			} finally {
				if (!cancelled) setLoading(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [query]);

	return (
		<Stack spacing={2}>
			<Typography variant="h5" component="h2">
				Search devices
			</Typography>
			<TextField
				type="search"
				fullWidth
				placeholder="Search by public key, key name, or connection IP…"
				value={query}
				onChange={(e) => setQuery(e.target.value)}
			/>
			{loading && <LinearProgress />}
			{error && <Alert severity="error">{error}</Alert>}
			{!loading && results !== null && results.length === 0 && query.trim() && (
				<Alert severity="info">No devices found matching your search.</Alert>
			)}
			{results && results.length > 0 && (
				<Stack spacing={1}>
					{results.map((d) => (
						<DeviceShorty key={d.device.id} device={d} />
					))}
				</Stack>
			)}
		</Stack>
	);
}
