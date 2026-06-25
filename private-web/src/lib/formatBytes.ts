/// Human-readable byte size (binary units). Returns "unknown" for a missing
/// value, so callers can pass an optional field straight through.
export function formatBytes(bytes: number | null | undefined): string {
	if (bytes == null) return "unknown";
	if (bytes < 1024) return `${bytes} B`;
	const units = ["KiB", "MiB", "GiB", "TiB"];
	let v = bytes / 1024;
	let i = 0;
	while (v >= 1024 && i < units.length - 1) {
		v /= 1024;
		i++;
	}
	return `${v.toFixed(1)} ${units[i]}`;
}
