export type Severity = 'critical' | 'error' | 'warning' | 'info';

export interface Finding {
	file: string;
	line: number;
	severity: Severity;
	title: string;
	body: string;
	confidence?: number;
	snippet?: string;
	category?: string;
	committable_patch?: string;
	feedback?: string;
}

export interface ReviewRecord {
	id: string;
	repo: string;
	pr_number: number;
	title?: string;
	status?: 'running' | 'completed' | 'failed' | 'pending';
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
	lines_added?: number;
	lines_removed?: number;
	executive_summary?: string;
	triage_status?: 'now' | 'next';
	risk_level?: 'low' | 'medium' | 'high';
}

export interface ProviderStatus {
	id?: string;
	name: string;
	type: string;
	model: string;
	status: string;
	cost_per_token: string;
	latency_avg: string;
}

export interface DaemonMetrics {
	total_reviews: number;
	pass1_candidates: number;
	pass2_filtered: number;
	verified_findings: number;
	avg_cost_saved: string;
	uptime: string;
}

export interface WizardConfig {
	profile: 'quiet' | 'chill' | 'assertive';
	auto_approvals: boolean;
	max_merge_risk: 'low' | 'medium' | 'high';
	high_level_summary: boolean;
	anti_slop: boolean;
	ignore_lockfiles: boolean;
	ignore_vendor: boolean;
	ignore_generated: boolean;
	custom_guidelines: string;
}

export interface SkillSummary {
	name: string;
	description: string;
	triggers: string[];
}

export interface FeedbackRecord {
	id: number;
	repo: string;
	file_path: string;
	issue_title: string;
	feedback_type: string;
	reason?: string;
	created_at: string;
}
