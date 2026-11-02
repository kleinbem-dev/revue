export type Severity = 'critical' | 'error' | 'warning' | 'info';

export interface Finding {
	file: string;
	line: number;
	severity: Severity;
	title: string;
	body: string;
	confidence?: number;
	snippet?: string;
}

export interface ReviewRecord {
	id: string;
	repo: string;
	pr_number: number;
	title?: string;
	author?: string;
	forge: 'github' | 'forgejo' | 'gitea';
	created_at: string;
	duration_secs: number;
	provider_used: string;
	findings_count: number;
	pass1_candidates: number;
	pass2_verified: number;
	summary: string;
	findings: Finding[];
}

export interface ProviderStatus {
	id: string;
	name: string;
	type: 'subscription' | 'api' | 'local_gpu';
	model: string;
	status: 'active' | 'ready' | 'fallback' | 'unconfigured';
	cost_per_token: string;
	latency_avg: string;
}

export interface DaemonMetrics {
	uptime_secs: number;
	total_reviews: number;
	estimated_savings_usd: number;
	active_provider: string;
	pass1_total_candidates: number;
	pass2_total_filtered: number;
}
