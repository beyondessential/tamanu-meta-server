import type { ServerKind, ServerRank } from "../types";

export interface CanopyTicket {
	v: string;
	serverId: string;
	publicKey: string;
	hostname: string;
	tailscaleIp?: string;
	tailscaleName?: string;
	canonicalUrl: string;
	hosting?: string;
	kind?: ServerKind;
	rank?: ServerRank;
	centralPublicKey?: string;
}

export function parseCanopyTicket(raw: string): CanopyTicket | null {
	const trimmed = raw.trim();
	if (!trimmed) return null;
	for (const candidate of base64Candidates(trimmed)) {
		try {
			const decoded = atob(candidate);
			const parsed = JSON.parse(decoded) as CanopyTicket;
			if (parsed && typeof parsed === "object" && parsed.v === "ticket-1") {
				return parsed;
			}
		} catch {
			// try next candidate
		}
	}
	return null;
}

function base64Candidates(s: string): string[] {
	// Mirror the Rust side: try standard, standard-no-pad, url-safe, url-safe-no-pad.
	const urlSafe = s.replace(/-/g, "+").replace(/_/g, "/");
	const padded = padBase64(s);
	const urlSafePadded = padBase64(urlSafe);
	return Array.from(new Set([s, padded, urlSafe, urlSafePadded]));
}

function padBase64(s: string): string {
	const remainder = s.length % 4;
	if (remainder === 0) return s;
	return s + "=".repeat(4 - remainder);
}
