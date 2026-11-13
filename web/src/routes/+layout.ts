import type { LayoutLoad } from './$types';
import {
	apiFetch,
	mapApiFinding,
	severityBucket,
	type DaemonMetrics,
	type ProviderStatus,
	type RawApiFinding,
	type ReviewRecord
} from '#lib';

export const ssr = false;
export const prerender = true;

let cachedMetrics: DaemonMetrics | null = null;
let cachedProviders: ProviderStatus[] | null = null;
let cachedReviews: ReviewRecord[] | null = null;
let cachedVersion = '0.1.0';
let cachedForgeUrl: string | null = null;

const initialDefaultMetrics: DaemonMetrics = {
	total_reviews: 0,
	pass1_candidates: 0,
	pass2_filtered: 0,
	verified_findings: 0,
	avg_cost_saved: '$0.00',
	uptime: '0m'
};

interface RawProviderStatus {
	id?: string;
	name?: string;
	type?: string;
	model?: string;
	status?: string;
	cost_per_token?: string;
	latency_avg?: string;
}

interface RawReviewRecord {
	id?: string | number;
	repo?: string;
	pr_number?: number;
	title?: string;
	status?: string;
	author?: string;
	forge?: string;
	created_at?: string;
	duration_secs?: number;
	provider_used?: string;
	pass1_candidates?: number;
	pass2_verified?: number;
	lines_added?: number;
	lines_removed?: number;
	summary?: string;
	findings?: RawApiFinding[];
}

export const load: LayoutLoad = async ({ fetch, depends }) => {
	depends('app:status');
	depends('app:reviews');

	let statusOk = false;
	let authRequired = false;

	async function loadStatus() {
		try {
			const statusRes = await fetch('/api/status', { signal: AbortSignal.timeout(5000) });
			if (statusRes.ok) {
				const data = await statusRes.json();
				if (typeof data?.version === 'string' && data.version.trim()) {
					cachedVersion = data.version.trim();
				}
				cachedForgeUrl =
					typeof data?.forge_url === 'string' && data.forge_url.trim()
						? data.forge_url.trim()
						: null;
				if (data?.metrics) {
					cachedMetrics = {
						total_reviews: Number(data.metrics.total_reviews ?? 0),
						pass1_candidates: Number(data.metrics.pass1_candidates ?? 0),
						pass2_filtered: Number(data.metrics.pass2_filtered ?? 0),
						verified_findings: Number(data.metrics.verified_findings ?? 0),
						avg_cost_saved: String(data.metrics.avg_cost_saved ?? '$0.00'),
						uptime: String(data.metrics.uptime ?? '0m')
					};
				}
				if (Array.isArray(data?.providers)) {
					cachedProviders = data.providers.map((p: RawProviderStatus) => ({
						id: String(p.id ?? p.name?.toLowerCase().replace(/\s+/g, '-') ?? 'unknown'),
						name: String(p.name ?? 'AI Provider'),
						type: String(p.type ?? 'api'),
						model: String(p.model ?? 'unknown'),
						status: String(p.status ?? 'configured'),
						cost_per_token: String(p.cost_per_token ?? '—'),
						latency_avg: String(p.latency_avg ?? '—')
					}));
				}
				statusOk = true;
			}
		} catch (err) {
			console.warn('Failed to refresh /api/status, retaining last known state:', err);
		}
	}

	async function loadReviews() {
		try {
			const reviewsRes = await apiFetch('/api/reviews', { signal: AbortSignal.timeout(5000) }, fetch);
			if (reviewsRes.status === 401 || reviewsRes.status === 403) {
				authRequired = true;
				return;
			}
			if (reviewsRes.ok) {
				const data: unknown = await reviewsRes.json();
				if (Array.isArray(data)) {
					cachedReviews = (data as RawReviewRecord[]).map((r) => {
						const findings = Array.isArray(r.findings) ? r.findings.map(mapApiFinding) : [];
						const hasCritical = findings.some((f) => severityBucket(f.severity) === 'critical');
						const hasMajor = findings.some((f) => severityBucket(f.severity) === 'major');
						const status = (r.status ?? 'completed') as
							| 'running'
							| 'completed'
							| 'failed'
							| 'pending';
						return {
							id: String(r.id ?? ''),
							repo: String(r.repo ?? 'unknown/repo'),
							pr_number: Number(r.pr_number ?? 0),
							title: r.title ? String(r.title) : undefined,
							status,
							author: r.author ? String(r.author) : 'system',
							forge: (r.forge ?? 'github') as 'github' | 'forgejo' | 'gitea',
							created_at: String(r.created_at ?? 'Recently'),
							duration_secs: Number(r.duration_secs ?? 0),
							provider_used: String(r.provider_used ?? 'Unknown'),
							findings_count: findings.length,
							pass1_candidates: Number(r.pass1_candidates ?? 0),
							pass2_verified: Number(r.pass2_verified ?? 0),
							lines_added: r.lines_added !== undefined ? Number(r.lines_added) : undefined,
							lines_removed: r.lines_removed !== undefined ? Number(r.lines_removed) : undefined,
							triage_status:
								status === 'failed' ||
								status === 'running' ||
								status === 'pending' ||
								findings.length > 0
									? 'now'
									: 'next',
							risk_level:
								status === 'failed' || hasCritical ? 'high' : hasMajor ? 'medium' : 'low',
							summary: String(r.summary ?? ''),
							findings
						};
					});
				}
			}
		} catch (err) {
			console.warn('Failed to refresh /api/reviews, retaining last known state:', err);
		}
	}

	await Promise.all([loadStatus(), loadReviews()]);

	return {
		version: cachedVersion,
		forgeUrl: cachedForgeUrl,
		metrics: cachedMetrics ?? initialDefaultMetrics,
		providers: cachedProviders ?? [],
		reviews: cachedReviews ?? [],
		statusOk,
		authRequired
	};
};
