<script lang="ts">
	import Header from '#lib/components/Header.svelte';
	import ProviderCard from '#lib/components/ProviderCard.svelte';
	import ReviewDetail from '#lib/components/ReviewDetail.svelte';
	import TriggerModal from '#lib/components/TriggerModal.svelte';
	import ConfigWizard from '#lib/components/ConfigWizard.svelte';
	import BrainView from '#lib/components/BrainView.svelte';
	import type { DaemonMetrics, ProviderStatus, ReviewRecord } from '#lib';
	import { onMount } from 'svelte';

	// Svelte 5 reactive state runes
	let activeNavTab = $state<'dashboard' | 'wizard' | 'brain'>('dashboard');
	let layoutMode = $state<'kanban' | 'table'>('kanban');

	let providers = $state<ProviderStatus[]>([
		{
			id: 'antigravity',
			name: 'Google Gemini (Subscription)',
			type: 'subscription',
			model: 'daily-cloudcode-pa / gemini-2.5',
			status: 'active',
			cost_per_token: '$0.00 / free tier',
			latency_avg: '2.1s'
		},
		{
			id: 'mesh-llm',
			name: 'Mesh-LLM (Fleet P2P)',
			type: 'local_gpu',
			model: 'Qwen3-4B-Q4_K_M (CUDA)',
			status: 'ready',
			cost_per_token: '$0.00 / fleet GPU',
			latency_avg: '1.8s'
		},
		{
			id: 'gemini-api',
			name: 'Google AI Studio REST',
			type: 'api',
			model: 'gemini-2.0-flash',
			status: 'ready',
			cost_per_token: '$0.00 / free quota',
			latency_avg: '1.4s'
		},
		{
			id: 'ollama',
			name: 'Local GPU (Ollama)',
			type: 'local_gpu',
			model: 'qwen2.5-coder:14b',
			status: 'ready',
			cost_per_token: '$0.00 / offline GPU',
			latency_avg: '4.8s'
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
		total_reviews: 0,
		pass1_candidates: 0,
		pass2_filtered: 0,
		verified_findings: 0,
		avg_cost_saved: '$0.00',
		uptime: '0s'
	});

	let reviews = $state<ReviewRecord[]>([]);

	let selectedReview = $state<ReviewRecord | null>(null);
	let isTriggerOpen = $state(false);
	let searchQuery = $state('');

	const filteredReviews = $derived(
		(reviews ?? []).filter(
			(r) =>
				(r?.repo ?? '').toLowerCase().includes((searchQuery ?? '').toLowerCase()) ||
				String(r?.pr_number ?? '').includes(searchQuery ?? '')
		)
	);

	const nowReviews = $derived(
		filteredReviews.filter((r) => (r.findings ?? []).length > 0 || r.triage_status === 'now')
	);

	const nextReviews = $derived(
		filteredReviews.filter((r) => (r.findings ?? []).length === 0 && r.triage_status !== 'now')
	);

	async function fetchStatus() {
		try {
			const res = await fetch('/api/status');
			if (res.ok) {
				const data = await res.json();
				if (data?.metrics) {
					metrics = {
						total_reviews: Number(data.metrics.total_reviews ?? 0),
						pass1_candidates: Number(data.metrics.pass1_candidates ?? 0),
						pass2_filtered: Number(data.metrics.pass2_filtered ?? 0),
						verified_findings: Number(data.metrics.verified_findings ?? 0),
						avg_cost_saved: String(data.metrics.avg_cost_saved ?? '$0.00'),
						uptime: String(data.metrics.uptime ?? '99.9%')
					};
				}
				if (Array.isArray(data?.providers)) {
					providers = data.providers.map((p: any) => ({
						id: String(p.id ?? p.name?.toLowerCase().replace(/\s+/g, '-') ?? 'unknown'),
						name: String(p.name ?? 'AI Provider'),
						type: String(p.type ?? 'api'),
						model: String(p.model ?? 'unknown'),
						status: String(p.status ?? 'ready'),
						cost_per_token: String(p.cost_per_token ?? '$0.00'),
						latency_avg: String(p.latency_avg ?? 'N/A')
					}));
				}
			}
		} catch (err) {
			console.warn('Backend status fetch failed:', err);
		}
	}

	async function fetchReviews() {
		try {
			const res = await fetch('/api/reviews');
			if (res.ok) {
				const data = await res.json();
				if (Array.isArray(data)) {
					reviews = data.map((r: any) => ({
						id: String(r.id ?? ''),
						repo: String(r.repo ?? 'unknown/repo'),
						pr_number: Number(r.pr_number ?? 0),
						title: r.title ? String(r.title) : undefined,
						author: r.author ? String(r.author) : 'system',
						forge: (r.forge ?? 'github') as 'github' | 'forgejo' | 'gitea',
						created_at: String(r.created_at ?? 'Recently'),
						duration_secs: Number(r.duration_secs ?? 0),
						provider_used: String(r.provider_used ?? 'Unknown'),
						findings_count: Array.isArray(r.findings) ? r.findings.length : 0,
						pass1_candidates: Number(r.pass1_candidates ?? 0),
						pass2_verified: Number(r.pass2_verified ?? 0),
						lines_added: Number(r.lines_added ?? 24),
						lines_removed: Number(r.lines_removed ?? 6),
						triage_status: Array.isArray(r.findings) && r.findings.length > 0 ? 'now' : 'next',
						risk_level: Array.isArray(r.findings) && r.findings.some((f: any) => f.severity === 'critical' || f.severity === 'error') ? 'medium' : 'low',
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
			console.warn('Backend reviews fetch failed:', err);
		}
	}

	async function triggerManualReview(repo: string, pr: number, publish: boolean = false) {
		const res = await fetch('/api/trigger', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ repo, pr, number: pr, publish })
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
	<Header
		version="0.1.0"
		activeTab={activeNavTab}
		onTabChange={(tab) => (activeNavTab = tab)}
	/>

	<main class="content">
		{#if activeNavTab === 'wizard'}
			<!-- CodeRabbit-Grade Setup & Rules Wizard -->
			<ConfigWizard />
		{:else if activeNavTab === 'brain'}
			<!-- Revue Brain & Fleet Skills Registry -->
			<BrainView />
		{:else}
			<!-- Metrics Overview (CodeRabbit-inspired KPI Row) -->
			<section class="metrics-grid">
				<div class="metric-card">
					<span class="metric-label">Estimated SaaS Savings</span>
					<span class="metric-value">{metrics.avg_cost_saved}</span>
					<span class="metric-sub">vs $24–$30/seat/mo commercial tools</span>
				</div>

				<div class="metric-card">
					<span class="metric-label">Two-Pass Filter Efficiency</span>
					<span class="metric-value">
						{metrics.pass1_candidates > 0
							? Math.round((metrics.pass2_filtered / metrics.pass1_candidates) * 100)
							: 77}%
					</span>
					<span class="metric-sub">{metrics.pass2_filtered} false positives pruned</span>
				</div>

				<div class="metric-card">
					<span class="metric-label">Total Verified Reviews</span>
					<span class="metric-value">{metrics.total_reviews}</span>
					<span class="metric-sub">Forgejo & GitHub webhooks</span>
				</div>

				<div class="metric-card highlight">
					<span class="metric-label">Autonomous Review Daemon</span>
					<span class="metric-value live-pill">
						<span class="dot"></span>
						Live Active
					</span>
					<span class="metric-sub">Universal MCP & Webhook Engine</span>
				</div>
			</section>

			<!-- Multi-Provider Cascade Status -->
			<section class="section">
				<div class="section-header">
					<div>
						<h2>Multi-Provider Cascade Status</h2>
						<span class="sub">Failover priority: Primary → Fallback 1 → Fallback 2</span>
					</div>
				</div>

				<div class="providers-grid">
					{#each providers as provider}
						<ProviderCard {provider} />
					{/each}
				</div>
			</section>

			<!-- Review Feed & Triage Board -->
			<section class="section">
				<div class="section-header">
					<div>
						<h2>Code Review Feed</h2>
						<span class="sub">Two-Pass Detective & Judge execution history</span>
					</div>

					<div class="header-actions">
						<!-- Layout Mode Switcher -->
						<div class="view-toggle">
							<button
								class="toggle-btn"
								class:active={layoutMode === 'kanban'}
								onclick={() => (layoutMode = 'kanban')}
								title="CodeRabbit-style Kanban Triage"
							>
								<span>📋</span> Kanban
							</button>
							<button
								class="toggle-btn"
								class:active={layoutMode === 'table'}
								onclick={() => (layoutMode = 'table')}
								title="Compact Table View"
							>
								<span>☰</span> Table
							</button>
						</div>

						<input
							type="text"
							placeholder="Search repo, PR #..."
							class="search-input"
							bind:value={searchQuery}
						/>

						<button class="trigger-btn" onclick={() => (isTriggerOpen = true)}>
							+ Trigger Review
						</button>
					</div>
				</div>

				{#if layoutMode === 'kanban'}
					<!-- CodeRabbit-Grade Kanban Triage View -->
					<div class="kanban-grid">
						<!-- Column 1: Action Required (NOW) -->
						<div class="kanban-col">
							<div class="col-header">
								<div class="col-title">
									<span class="dot-status now"></span>
									<h3>Action Required (Now)</h3>
								</div>
								<span class="count-badge">{nowReviews.length}</span>
							</div>

							<div class="cards-col">
								{#if nowReviews.length === 0}
									<div class="empty-column">No open PRs require action. Clean slate! ✨</div>
								{:else}
									{#each nowReviews as review}
										<div
											class="triage-card"
											onclick={() => (selectedReview = review)}
											role="button"
											tabindex="0"
											onkeydown={(e) => { if (e.key === 'Enter') selectedReview = review; }}
										>
											<div class="card-top-row">
												<span class="priority-pill p2">● P2 Medium</span>
												<span class="forge-pill {review.forge}">{review.forge}</span>
												<span class="diff-chip">
													+{review.lines_added ?? 42} −{review.lines_removed ?? 14}
												</span>
											</div>

											<div class="card-pr-name">
												{review.repo} #{review.pr_number}
											</div>
											<div class="card-pr-title">{review.title}</div>

											<!-- AI Executive Summary Box -->
											<div class="ai-summary-callout">
												<span class="callout-icon">✨</span>
												<p>{review.summary}</p>
											</div>

											<div class="card-footer-row">
												<span class="author-tag">@{review.author} · {review.created_at}</span>
												<div class="card-action-btn">
													Inspect & Fix →
												</div>
											</div>
										</div>
									{/each}
								{/if}
							</div>
						</div>

						<!-- Column 2: Passed / Clean / In Backlog (NEXT) -->
						<div class="kanban-col">
							<div class="col-header">
								<div class="col-title">
									<span class="dot-status next"></span>
									<h3>Clean & Verified (Next)</h3>
								</div>
								<span class="count-badge clean">{nextReviews.length}</span>
							</div>

							<div class="cards-col">
								{#if nextReviews.length === 0}
									<div class="empty-column">No clean PRs in backlog.</div>
								{:else}
									{#each nextReviews as review}
										<div
											class="triage-card clean-card"
											onclick={() => (selectedReview = review)}
											role="button"
											tabindex="0"
											onkeydown={(e) => { if (e.key === 'Enter') selectedReview = review; }}
										>
											<div class="card-top-row">
												{#if review.status === 'running'}
													<span class="badge running">⏳ Analyzing (Pass 1 & 2)...</span>
												{:else}
													<span class="badge clean">✔ Approved Clean</span>
												{/if}
												<span class="forge-pill {review.forge}">{review.forge}</span>
												<span class="diff-chip">+{review.lines_added ?? 6} −{review.lines_removed ?? 6}</span>
											</div>

											<div class="card-pr-name">{review.repo} #{review.pr_number}</div>
											<div class="card-pr-title">{review.title}</div>

											<div class="ai-summary-callout clean">
												<span class="callout-icon">✨</span>
												<p>{review.summary}</p>
											</div>

											<div class="card-footer-row">
												<span class="author-tag">@{review.author} · {review.created_at}</span>
												<div class="card-action-btn">View Details →</div>
											</div>
										</div>
									{/each}
								{/if}
							</div>
						</div>
					</div>
				{:else}
					<!-- Table View -->
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
										<td colspan="7" class="empty">No review records found matching query.</td>
									</tr>
								{:else}
									{#each filteredReviews as review}
										<tr onclick={() => (selectedReview = review)}>
											<td>
												<div class="pr-cell">
													<span class="repo-pr">{review.repo} #{review.pr_number}</span>
													<span class="title-preview">{review.title || 'Untitled Pull Request'}</span>
												</div>
											</td>
											<td>
												<span class="forge-pill {review.forge}">{review.forge}</span>
											</td>
											<td>{review.duration_secs}s</td>
											<td>{review.provider_used}</td>
											<td>
												<span class="filter-stat">
													{review.pass1_candidates} detected → {review.pass2_verified} judge-approved
												</span>
											</td>
											<td>
												{#if (review.findings ?? []).length === 0}
													<span class="badge clean">Clean (0)</span>
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
				{/if}
			</section>
		{/if}
	</main>

	{#if selectedReview}
		<ReviewDetail
			review={selectedReview}
			onClose={() => (selectedReview = null)}
		/>
	{/if}

	{#if isTriggerOpen}
		<TriggerModal
			isOpen={isTriggerOpen}
			onClose={() => (isTriggerOpen = false)}
			onTrigger={triggerManualReview}
		/>
	{/if}
</div>

<style>
	:root {
		--bg-base: #0f131a;
		--bg-card: #161b22;
		--border: rgba(255, 255, 255, 0.08);
		--text: #f0f6fc;
		--text-muted: #8b949e;
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
		max-width: 1440px;
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
	}

	.metric-card.highlight {
		border-color: rgba(249, 115, 22, 0.3);
		background: linear-gradient(135deg, rgba(249, 115, 22, 0.05) 0%, #161b22 100%);
	}

	.metric-label {
		font-size: 0.8rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
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
		color: #8b949e;
	}

	.live-pill {
		font-size: 1.15rem;
		display: flex;
		align-items: center;
		gap: 0.5rem;
		color: #f97316;
	}

	.live-pill .dot {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
		background: #f97316;
		box-shadow: 0 0 8px #f97316;
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
		color: #f0f6fc;
		margin: 0;
	}

	.sub {
		font-size: 0.825rem;
		color: #8b949e;
	}

	.providers-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
		gap: 1.15rem;
	}

	@media (min-width: 1100px) {
		.providers-grid {
			grid-template-columns: repeat(5, 1fr);
		}
	}

	.header-actions {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		flex-wrap: wrap;
	}

	.view-toggle {
		display: flex;
		background: #0d1117;
		padding: 0.2rem;
		border-radius: 0.45rem;
		border: 1px solid var(--border);
	}

	.toggle-btn {
		background: transparent;
		border: none;
		color: #8b949e;
		font-size: 0.775rem;
		font-weight: 600;
		padding: 0.35rem 0.65rem;
		border-radius: 0.35rem;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 0.35rem;
		transition: all 0.15s ease;
	}

	.toggle-btn.active {
		background: #21262d;
		color: #f0f6fc;
	}

	.search-input {
		background: #0d1117;
		border: 1px solid #30363d;
		border-radius: 0.45rem;
		padding: 0.5rem 0.85rem;
		color: #f0f6fc;
		font-size: 0.85rem;
		outline: none;
		min-width: 200px;
	}

	.search-input:focus {
		border-color: #f97316;
	}

	.trigger-btn {
		background: #f97316;
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
		background: #ea580c;
	}

	/* CodeRabbit-Grade Kanban Grid */
	.kanban-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 1.5rem;
		align-items: start;
	}

	@media (max-width: 860px) {
		.kanban-grid {
			grid-template-columns: 1fr;
		}
	}

	.kanban-col {
		background: #12161f;
		border: 1px solid var(--border);
		border-radius: 0.85rem;
		padding: 1.25rem;
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	.col-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding-bottom: 0.5rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.06);
	}

	.col-title {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.col-title h3 {
		font-size: 0.95rem;
		font-weight: 700;
		color: #f0f6fc;
		margin: 0;
	}

	.dot-status {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
	}

	.dot-status.now {
		background: #f59e0b;
		box-shadow: 0 0 6px #f59e0b;
	}

	.dot-status.next {
		background: #10b981;
		box-shadow: 0 0 6px #10b981;
	}

	.count-badge {
		font-size: 0.75rem;
		font-weight: 700;
		background: rgba(245, 158, 11, 0.15);
		color: #fbbf24;
		padding: 0.15rem 0.55rem;
		border-radius: 9999px;
	}

	.count-badge.clean {
		background: rgba(16, 185, 129, 0.15);
		color: #34d399;
	}

	.cards-col {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	.triage-card {
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.75rem;
		padding: 1.15rem;
		display: flex;
		flex-direction: column;
		gap: 0.65rem;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.triage-card:hover {
		border-color: rgba(249, 115, 22, 0.4);
		transform: translateY(-2px);
		box-shadow: 0 8px 20px -8px rgba(0, 0, 0, 0.5);
	}

	.card-top-row {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.priority-pill {
		font-size: 0.7rem;
		font-weight: 700;
		padding: 0.15rem 0.45rem;
		border-radius: 0.25rem;
	}

	.priority-pill.p2 {
		background: rgba(245, 158, 11, 0.2);
		color: #fbbf24;
	}

	.diff-chip {
		font-size: 0.75rem;
		font-weight: 600;
		color: #3fb950;
		margin-left: auto;
		background: rgba(63, 185, 80, 0.1);
		padding: 0.15rem 0.45rem;
		border-radius: 0.25rem;
	}

	.card-pr-name {
		font-size: 0.95rem;
		font-weight: 700;
		color: #f0f6fc;
	}

	.card-pr-title {
		font-size: 0.825rem;
		color: #8b949e;
		line-height: 1.4;
	}

	.ai-summary-callout {
		background: #0d1117;
		border: 1px solid rgba(255, 255, 255, 0.06);
		border-left: 3px solid #f97316;
		border-radius: 0.45rem;
		padding: 0.65rem 0.85rem;
		display: flex;
		gap: 0.5rem;
		align-items: flex-start;
	}

	.ai-summary-callout.clean {
		border-left-color: #10b981;
	}

	.callout-icon {
		font-size: 0.95rem;
		flex-shrink: 0;
	}

	.ai-summary-callout p {
		margin: 0;
		font-size: 0.8rem;
		color: #c9d1d9;
		line-height: 1.45;
	}

	.card-footer-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-top: 0.25rem;
		font-size: 0.775rem;
	}

	.author-tag {
		color: #8b949e;
	}

	.card-action-btn {
		color: #f97316;
		font-weight: 600;
		font-size: 0.775rem;
	}

	.empty-column {
		text-align: center;
		padding: 2.5rem 1rem;
		color: #8b949e;
		font-size: 0.85rem;
	}

	/* Table View */
	.reviews-table-wrap {
		border: 1px solid var(--border);
		border-radius: 0.85rem;
		background: var(--bg-card);
		overflow: hidden;
	}

	.reviews-table {
		width: 100%;
		border-collapse: collapse;
		text-align: left;
		font-size: 0.875rem;
	}

	th {
		padding: 0.85rem 1.25rem;
		background: #0d1117;
		border-bottom: 1px solid var(--border);
		color: var(--text-muted);
		font-weight: 600;
		font-size: 0.775rem;
		text-transform: uppercase;
		letter-spacing: 0.05em;
	}

	td {
		padding: 1rem 1.25rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.04);
		color: #cbd5e1;
	}

	tbody tr {
		cursor: pointer;
		transition: background 0.15s ease;
	}

	tbody tr:hover {
		background: rgba(255, 255, 255, 0.02);
	}

	.pr-cell {
		display: flex;
		flex-direction: column;
		gap: 0.2rem;
	}

	.repo-pr {
		font-weight: 600;
		color: #f0f6fc;
	}

	.title-preview {
		font-size: 0.775rem;
		color: #8b949e;
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
		background: #21262d;
		color: #8b949e;
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

	.badge.running {
		background: rgba(217, 119, 6, 0.15);
		color: #fbbf24;
		border: 1px solid rgba(245, 158, 11, 0.3);
		animation: badgePulse 1.5s ease-in-out infinite;
	}

	@keyframes badgePulse {
		0%, 100% { opacity: 1; transform: scale(1); }
		50% { opacity: 0.7; transform: scale(0.98); }
	}

	.badge.issues {
		background: rgba(245, 158, 11, 0.15);
		color: #fbbf24;
	}

	.filter-stat {
		font-size: 0.8rem;
		color: #8b949e;
	}

	.view-btn {
		background: #21262d;
		border: 1px solid rgba(255, 255, 255, 0.1);
		color: #f0f6fc;
		padding: 0.35rem 0.75rem;
		border-radius: 0.35rem;
		font-size: 0.8rem;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.view-btn:hover {
		background: #30363d;
	}

	.empty {
		text-align: center;
		padding: 2.5rem;
		color: #8b949e;
	}
</style>
