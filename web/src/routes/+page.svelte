<script lang="ts">
	import Header from '#lib/components/Header.svelte';
	import ProviderCard from '#lib/components/ProviderCard.svelte';
	import ReviewDetail from '#lib/components/ReviewDetail.svelte';
	import TriggerModal from '#lib/components/TriggerModal.svelte';
	import type { DaemonMetrics, ProviderStatus, ReviewRecord } from '#lib';
	import { onMount } from 'svelte';

	// Svelte 5 reactive state runes
	let providers = $state<ProviderStatus[]>([
		{
			id: 'antigravity',
			name: 'Google Gemini (Subscription)',
			type: 'subscription',
			model: 'daily-cloudcode-pa / gemini-2.5',
			status: 'active',
			cost_per_token: '$0.00 / free tier',
			latency_avg: '14.2s'
		},
		{
			id: 'gemini-api',
			name: 'Google AI Studio REST',
			type: 'api',
			model: 'gemini-2.0-flash',
			status: 'ready',
			cost_per_token: '$0.00 / free quota',
			latency_avg: '12.8s'
		},
		{
			id: 'ollama',
			name: 'Local GPU (Ollama)',
			type: 'local_gpu',
			model: 'qwen2.5-coder:14b',
			status: 'ready',
			cost_per_token: '$0.00 / offline GPU',
			latency_avg: '28.5s'
		},
		{
			id: 'claude',
			name: 'Claude Code CLI',
			type: 'subscription',
			model: 'claude-3-5-sonnet',
			status: 'fallback',
			cost_per_token: 'Pro/Team quota protected',
			latency_avg: '18.1s'
		}
	]);

	let metrics = $state<DaemonMetrics>({
		uptime_secs: 14280,
		total_reviews: 24,
		estimated_savings_usd: 720.0,
		active_provider: 'Google Gemini (Subscription)',
		pass1_total_candidates: 184,
		pass2_total_filtered: 142
	});

	let reviews = $state<ReviewRecord[]>([
		{
			id: 'rev-01',
			repo: 'kleinbem/nix-config',
			pr_number: 505,
			title: 'feat(fleet): enable Forgejo on mac-mini and revue daemon on nixos-nvme',
			author: 'martin',
			forge: 'github',
			created_at: 'Just now',
			duration_secs: 34,
			provider_used: 'Google Gemini (Subscription)',
			findings_count: 0,
			pass1_candidates: 2,
			pass2_verified: 0,
			summary:
				'Evaluated fleet configurations across mac-mini and nixos-nvme. Zero-Trust firewall ports and container routing verified.',
			findings: []
		},
		{
			id: 'rev-02',
			repo: 'kleinbem/nix-config',
			pr_number: 489,
			title: 'fix(network): hass-pi hairpin routing and Music Assistant ports',
			author: 'martin',
			forge: 'forgejo',
			created_at: '2 hours ago',
			duration_secs: 31,
			provider_used: 'Google Gemini (Subscription)',
			findings_count: 2,
			pass1_candidates: 5,
			pass2_verified: 2,
			summary:
				'Pass 1 identified 5 anomalies in the bridge NAT rules. Pass 2 Judge confirmed asymmetric routing failure on container bridge cbr0 and missing Zero-Trust rules for Music Assistant ports 8095/8097, filtering out 3 false positives.',
			findings: [
				{
					file: 'hosts/hass-pi/default.nix',
					line: 42,
					severity: 'error',
					title: 'Hairpin NAT missing masquerade rule on cbr0',
					body: 'Packets hairpinned back to container bridge cbr0 will retain the original source IP, causing container TCP connections to be rejected due to asymmetric return path.',
					confidence: 9,
					snippet: 'nft add rule ip nat POSTROUTING oifname "cbr0" masquerade'
				},
				{
					file: 'hosts/hass-pi/network.nix',
					line: 78,
					severity: 'warning',
					title: 'Music Assistant airlock missing stream ports',
					body: 'AirPlay and DLNA announcement ports (8095, 8097) are omitted from the firewall whitelist.',
					confidence: 8,
					snippet: 'interfaces."cbr0".allowedTCPPorts = [ 8095 8097 ];'
				}
			]
		}
	]);

	let selectedReview = $state<ReviewRecord | null>(null);
	let isTriggerOpen = $state(false);
	let searchQuery = $state('');

	const filteredReviews = $derived(
		reviews.filter(
			(r) =>
				r.repo.toLowerCase().includes(searchQuery.toLowerCase()) ||
				String(r.pr_number).includes(searchQuery)
		)
	);

	async function fetchStatus() {
		try {
			const res = await fetch('/api/status');
			if (res.ok) {
				const data = await res.json();
				if (data.metrics) metrics = data.metrics;
				if (data.providers) providers = data.providers;
			}
		} catch {
			// Backend offline in dev, keep initial data
		}
	}

	async function fetchReviews() {
		try {
			const res = await fetch('/api/reviews');
			if (res.ok) {
				const data = await res.json();
				if (Array.isArray(data)) reviews = data;
			}
		} catch {
			// Backend offline in dev, keep initial data
		}
	}

	async function triggerManualReview(repo: string, pr: number) {
		const res = await fetch('/api/trigger', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ repo, number: pr })
		});
		if (!res.ok) {
			const text = await res.text();
			throw new Error(text || 'Failed to queue review');
		}
		await fetchReviews();
	}

	onMount(() => {
		fetchStatus();
		fetchReviews();
		const interval = setInterval(() => {
			fetchStatus();
			fetchReviews();
		}, 10000);
		return () => clearInterval(interval);
	});
</script>

<svelte:head>
	<title>Revue Dashboard — AI Code Review</title>
</svelte:head>

<div class="layout">
	<Header version="0.1.0" />

	<main class="content">
		<!-- Metrics Overview -->
		<section class="metrics-grid">
			<div class="metric-card">
				<span class="metric-label">Total Reviews</span>
				<div class="metric-value">{metrics.total_reviews}</div>
				<span class="metric-sub">Forgejo & GitHub</span>
			</div>
			<div class="metric-card highlight">
				<span class="metric-label">Estimated SaaS Savings</span>
				<div class="metric-value">${metrics.estimated_savings_usd.toFixed(0)}</div>
				<span class="metric-sub">vs $24–$30/seat/mo CodeRabbit</span>
			</div>
			<div class="metric-card">
				<span class="metric-label">Judge Filter Rate</span>
				<div class="metric-value">
					{Math.round((metrics.pass2_total_filtered / metrics.pass1_total_candidates) * 100)}%
				</div>
				<span class="metric-sub">{metrics.pass2_total_filtered} false positives pruned</span>
			</div>
			<div class="metric-card">
				<span class="metric-label">Primary AI Provider</span>
				<div class="metric-value live-pill">
					<span class="dot"></span>
					Gemini Sub ($0)
				</div>
				<span class="metric-sub">Daily Cloud Code CLI</span>
			</div>
		</section>

		<!-- Provider Cascade Health -->
		<section class="section">
			<div class="section-header">
				<h2>Multi-Provider Cascade Status</h2>
				<span class="sub">Failover priority: Primary $\rightarrow$ Fallback 1 $\rightarrow$ Fallback 2</span>
			</div>
			<div class="providers-grid">
				{#each providers as provider}
					<ProviderCard {provider} />
				{/each}
			</div>
		</section>

		<!-- Recent Activity Feed -->
		<section class="section">
			<div class="section-header">
				<div class="header-left">
					<h2>Recent Code Reviews</h2>
					<span class="sub">Two-Pass Detective & Judge execution history</span>
				</div>
				<div class="header-actions">
					<input
						type="search"
						bind:value={searchQuery}
						placeholder="Filter by repo or PR #..."
						class="search-input"
					/>
					<button class="trigger-btn" onclick={() => (isTriggerOpen = true)}>
						+ Trigger Review
					</button>
				</div>
			</div>

			<div class="reviews-table-wrap">
				<table class="reviews-table">
					<thead>
						<tr>
							<th>Pull Request</th>
							<th>Forge</th>
							<th>Duration</th>
							<th>Engine</th>
							<th>Two-Pass Filtering</th>
							<th>Findings</th>
							<th>Action</th>
						</tr>
					</thead>
					<tbody>
						{#if filteredReviews.length === 0}
							<tr>
								<td colspan="7" class="empty">No matching review records found.</td>
							</tr>
						{:else}
							{#each filteredReviews as review}
								<tr onclick={() => (selectedReview = review)}>
									<td>
										<div class="pr-cell">
											<span class="repo-pr">{review.repo} #{review.pr_number}</span>
											{#if review.title}
												<span class="title-preview">{review.title}</span>
											{/if}
										</div>
									</td>
									<td>
										<span class="forge-pill {review.forge}">{review.forge}</span>
									</td>
									<td>
										<span class="duration">{review.duration_secs}s</span>
									</td>
									<td>
										<span class="engine">{review.provider_used}</span>
									</td>
									<td>
										<span class="filter-stat">
											{review.pass1_candidates} detected $\rightarrow$ {review.pass2_verified} judge-approved
										</span>
									</td>
									<td>
										{#if review.findings.length === 0}
											<span class="badge clean">Clean ✨</span>
										{:else}
											<span class="badge issues">{review.findings.length} flagged</span>
										{/if}
									</td>
									<td>
										<button
											class="view-btn"
											onclick={(e) => {
												e.stopPropagation();
												selectedReview = review;
											}}
										>
											Inspect
										</button>
									</td>
								</tr>
							{/each}
						{/if}
					</tbody>
				</table>
			</div>
		</section>
	</main>

	<!-- Detailed Review Modal -->
	{#if selectedReview}
		<ReviewDetail review={selectedReview} onClose={() => (selectedReview = null)} />
	{/if}

	<!-- Manual Trigger Modal -->
	<TriggerModal
		isOpen={isTriggerOpen}
		onClose={() => (isTriggerOpen = false)}
		onTrigger={triggerManualReview}
	/>
</div>

<style>
	:global(:root) {
		--bg-base: #090e17;
		--bg-card: rgba(15, 23, 42, 0.65);
		--border: #1e293b;
		--text: #f8fafc;
		--text-muted: #94a3b8;
	}

	:global(body) {
		margin: 0;
		background-color: var(--bg-base);
		color: var(--text);
		font-family:
			system-ui,
			-apple-system,
			BlinkMacSystemFont,
			'Segoe UI',
			Roboto,
			Oxygen,
			Ubuntu,
			Cantarell,
			sans-serif;
		-webkit-font-smoothing: antialiased;
	}

	.layout {
		min-height: 100vh;
		display: flex;
		flex-direction: column;
	}

	.content {
		max-width: 1200px;
		width: 100%;
		margin: 0 auto;
		padding: 2rem;
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		gap: 2.25rem;
	}

	.metrics-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
		gap: 1.25rem;
	}

	.metric-card {
		background: var(--bg-card);
		border: 1px solid var(--border);
		border-radius: 0.85rem;
		padding: 1.25rem 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
		backdrop-filter: blur(8px);
	}

	.metric-card.highlight {
		border-color: rgba(16, 185, 129, 0.3);
		background: linear-gradient(135deg, rgba(16, 185, 129, 0.05) 0%, rgba(15, 23, 42, 0.8) 100%);
	}

	.metric-label {
		font-size: 0.8rem;
		font-weight: 500;
		color: var(--text-muted);
	}

	.metric-value {
		font-size: 1.65rem;
		font-weight: 700;
		color: var(--text);
		letter-spacing: -0.02em;
	}

	.metric-sub {
		font-size: 0.775rem;
		color: #64748b;
	}

	.live-pill {
		font-size: 1.15rem;
		display: flex;
		align-items: center;
		gap: 0.5rem;
		color: #34d399;
	}

	.live-pill .dot {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
		background: #10b981;
		box-shadow: 0 0 8px #10b981;
	}

	.section {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	.section-header {
		display: flex;
		align-items: flex-end;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 1rem;
	}

	h2 {
		font-size: 1.2rem;
		font-weight: 700;
		color: #f1f5f9;
		margin: 0;
	}

	.sub {
		font-size: 0.825rem;
		color: #64748b;
	}

	.providers-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
		gap: 1.15rem;
	}

	.header-actions {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.search-input {
		background: rgba(15, 23, 42, 0.8);
		border: 1px solid #334155;
		border-radius: 0.45rem;
		padding: 0.5rem 0.85rem;
		color: #f8fafc;
		font-size: 0.85rem;
		outline: none;
		min-width: 220px;
	}

	.search-input:focus {
		border-color: #8b5cf6;
	}

	.trigger-btn {
		background: #8b5cf6;
		color: white;
		border: none;
		border-radius: 0.45rem;
		padding: 0.5rem 1rem;
		font-size: 0.85rem;
		font-weight: 600;
		cursor: pointer;
		transition: background 0.15s ease;
	}

	.trigger-btn:hover {
		background: #7c3aed;
	}

	.reviews-table-wrap {
		border: 1px solid var(--border);
		border-radius: 0.85rem;
		background: var(--bg-card);
		overflow: hidden;
		backdrop-filter: blur(8px);
	}

	.reviews-table {
		width: 100%;
		border-collapse: collapse;
		text-align: left;
		font-size: 0.875rem;
	}

	th {
		padding: 0.85rem 1.25rem;
		background: rgba(15, 23, 42, 0.8);
		border-bottom: 1px solid var(--border);
		color: var(--text-muted);
		font-weight: 600;
		font-size: 0.775rem;
		text-transform: uppercase;
		letter-spacing: 0.05em;
	}

	td {
		padding: 1rem 1.25rem;
		border-bottom: 1px solid rgba(30, 41, 59, 0.5);
		color: #cbd5e1;
	}

	tbody tr {
		cursor: pointer;
		transition: background 0.15s ease;
	}

	tbody tr:hover {
		background: rgba(30, 41, 59, 0.4);
	}

	.pr-cell {
		display: flex;
		flex-direction: column;
		gap: 0.2rem;
	}

	.repo-pr {
		font-weight: 600;
		color: #f8fafc;
	}

	.title-preview {
		font-size: 0.775rem;
		color: #64748b;
		max-width: 320px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.forge-pill {
		font-size: 0.7rem;
		font-weight: 600;
		padding: 0.15rem 0.5rem;
		border-radius: 9999px;
		text-transform: uppercase;
		background: #1e293b;
		color: #94a3b8;
	}

	.forge-pill.forgejo {
		background: rgba(16, 185, 129, 0.15);
		color: #34d399;
	}

	.badge {
		font-size: 0.75rem;
		font-weight: 600;
		padding: 0.2rem 0.6rem;
		border-radius: 9999px;
	}

	.badge.clean {
		background: rgba(16, 185, 129, 0.15);
		color: #34d399;
	}

	.badge.issues {
		background: rgba(245, 158, 11, 0.15);
		color: #fbbf24;
	}

	.filter-stat {
		font-size: 0.8rem;
		color: #94a3b8;
	}

	.view-btn {
		background: #1e293b;
		border: 1px solid #334155;
		color: #cbd5e1;
		padding: 0.35rem 0.75rem;
		border-radius: 0.35rem;
		font-size: 0.8rem;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.view-btn:hover {
		background: #334155;
		color: #f8fafc;
	}

	.empty {
		text-align: center;
		padding: 2.5rem;
		color: #64748b;
	}
</style>
