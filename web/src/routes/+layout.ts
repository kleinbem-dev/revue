import type { LayoutLoad } from './$types';
import type { DaemonMetrics, ProviderStatus, ReviewRecord } from '#lib';

export const ssr = false;
export const prerender = true;

// Preserve last successful states across periodic refreshes
let cachedMetrics: DaemonMetrics | null = null;
let cachedProviders: ProviderStatus[] | null = null;
let cachedReviews: ReviewRecord[] | null = null;

const initialDefaultProviders: ProviderStatus[] = [
	{
		id: 'antigravity',
		name: 'Google Gemini (Subscription)',
		type: 'subscription',
		model: 'daily-cloudcode-pa / gemini-2.5',
		status: 'unknown',
		cost_per_token: '$0.00 / free tier',
		latency_avg: 'N/A'
	},
	{
		id: 'mesh-llm',
		name: 'Mesh-LLM (Fleet P2P)',
		type: 'local_gpu',
		model: 'Qwen3-4B-Q4_K_M (CUDA)',
		status: 'unknown',
		cost_per_token: '$0.00 / fleet GPU',
		latency_avg: 'N/A'
	},
	{
		id: 'gemini-api',
		name: 'Google AI Studio REST',
		type: 'api',
		model: 'gemini-2.0-flash',
		status: 'unknown',
		cost_per_token: '$0.00 / free quota',
		latency_avg: 'N/A'
	},
	{
		id: 'ollama',
		name: 'Local GPU (Ollama)',
		type: 'local_gpu',
		model: 'qwen2.5-coder:14b',
		status: 'unknown',
		cost_per_token: '$0.00 / offline GPU',
		latency_avg: 'N/A'
	},
	{
		id: 'claude',
		name: 'Claude Code CLI',
		type: 'subscription',
		model: 'claude-3-5-sonnet',
		status: 'unknown',
		cost_per_token: 'Pro/Team quota protected',
		latency_avg: 'N/A'
	}
];

const initialDefaultMetrics: DaemonMetrics = {
	total_reviews: 0,
	pass1_candidates: 0,
	pass2_filtered: 0,
	verified_findings: 0,
	avg_cost_saved: '$0.00',
	uptime: '0s'
};

export const load: LayoutLoad = async ({ fetch, depends }) => {
	depends('app:status');
	depends('app:reviews');

	let statusOk = false;

	async function loadStatus() {
		try {
			const statusRes = await fetch('/api/status', { signal: AbortSignal.timeout(5000) });
			if (statusRes.ok) {
				const data = await statusRes.json();
				if (data?.metrics) {
					cachedMetrics = {
						total_reviews: Number(data.metrics.total_reviews ?? 0),
						pass1_candidates: Number(data.metrics.pass1_candidates ?? 0),
						pass2_filtered: Number(data.metrics.pass2_filtered ?? 0),
						verified_findings: Number(data.metrics.verified_findings ?? 0),
						avg_cost_saved: String(data.metrics.avg_cost_saved ?? '$0.00'),
						uptime: String(data.metrics.uptime ?? '0s')
					};
				}
				if (Array.isArray(data?.providers)) {
					cachedProviders = data.providers.map((p: any) => ({
						id: String(p.id ?? p.name?.toLowerCase().replace(/\s+/g, '-') ?? 'unknown'),
						name: String(p.name ?? 'AI Provider'),
						type: String(p.type ?? 'api'),
						model: String(p.model ?? 'unknown'),
						status: String(p.status ?? 'ready'),
						cost_per_token: String(p.cost_per_token ?? '$0.00'),
						latency_avg: String(p.latency_avg ?? 'N/A')
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
			const reviewsRes = await fetch('/api/reviews', { signal: AbortSignal.timeout(5000) });
			if (reviewsRes.ok) {
				const data = await reviewsRes.json();
				if (Array.isArray(data)) {
					cachedReviews = data.map((r: any) => ({
						id: String(r.id ?? ''),
						repo: String(r.repo ?? 'unknown/repo'),
						pr_number: Number(r.pr_number ?? 0),
						title: r.title ? String(r.title) : undefined,
						status: (r.status ?? 'completed') as 'running' | 'completed' | 'failed' | 'pending',
						author: r.author ? String(r.author) : 'system',
						forge: (r.forge ?? 'github') as 'github' | 'forgejo' | 'gitea',
						created_at: String(r.created_at ?? 'Recently'),
						duration_secs: Number(r.duration_secs ?? 0),
						provider_used: String(r.provider_used ?? 'Unknown'),
						findings_count: Array.isArray(r.findings) ? r.findings.length : 0,
						pass1_candidates: Number(r.pass1_candidates ?? 0),
						pass2_verified: Number(r.pass2_verified ?? 0),
						lines_added: r.lines_added !== undefined ? Number(r.lines_added) : undefined,
						lines_removed: r.lines_removed !== undefined ? Number(r.lines_removed) : undefined,
						triage_status:
							r.status === 'failed' ||
							r.status === 'running' ||
							r.status === 'pending' ||
							(Array.isArray(r.findings) && r.findings.length > 0)
								? 'now'
								: 'next',
						risk_level:
							r.status === 'failed'
								? 'high'
								: Array.isArray(r.findings) &&
								    r.findings.some((f: any) => f.severity === 'critical' || f.severity === 'error')
								  ? 'medium'
								  : 'low',
						summary: String(r.summary ?? ''),
						findings: Array.isArray(r.findings)
							? r.findings.map((f: any) => ({
									file: String(f.file ?? ''),
									line: Number(f.line ?? 0),
									severity: f.severity ?? 'info',
									title: String(f.title ?? ''),
									body: String(f.body ?? ''),
									confidence: f.confidence !== undefined ? Number(f.confidence) : undefined,
									snippet: f.snippet ? String(f.snippet) : undefined,
									category: f.category ?? 'Functional Correctness'
							  }))
							: []
					}));
				}
			}
		} catch (err) {
			console.warn('Failed to refresh /api/reviews, retaining last known state:', err);
		}
	}

	await Promise.all([loadStatus(), loadReviews()]);

	return {
		metrics: cachedMetrics ?? initialDefaultMetrics,
		providers: cachedProviders ?? initialDefaultProviders,
		reviews: cachedReviews ?? [],
		statusOk
	};
};
