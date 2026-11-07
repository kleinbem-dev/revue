<script lang="ts">
	import ProviderCard from '#lib/components/ProviderCard.svelte';
	import type { ReviewRecord, ProviderStatus, DaemonMetrics } from '#lib';
	import type { PageData } from './$types';
	import { page } from '$app/state';
	import { pushState, invalidate } from '$app/navigation';
	import { onMount } from 'svelte';

	let { data }: { data: PageData } = $props();

	let layoutMode = $state<'kanban' | 'table'>('kanban');
	let searchQuery = $state('');

	const reviews = $derived<ReviewRecord[]>(data.reviews ?? []);
	const metrics = $derived<DaemonMetrics>(data.metrics);
	const providers = $derived<ProviderStatus[]>(data.providers);

	const filteredReviews = $derived(
		reviews.filter(
			(r: ReviewRecord) =>
				(r?.repo ?? '').toLowerCase().includes((searchQuery ?? '').toLowerCase()) ||
				String(r?.pr_number ?? '').includes(searchQuery ?? '')
		)
	);

	const nowReviews = $derived(
		filteredReviews.filter((r: ReviewRecord) => (r.findings ?? []).length > 0 || r.triage_status === 'now')
	);

	const nextReviews = $derived(
		filteredReviews.filter((r: ReviewRecord) => (r.findings ?? []).length === 0 && r.triage_status !== 'now')
	);

	function openReview(review: ReviewRecord) {
		pushState(`?review=${review.id}`, { selectedReviewId: review.id });
	}

	function openTriggerModal() {
		pushState('', { isTriggerOpen: true });
	}

	onMount(() => {
		const interval = setInterval(() => {
			invalidate('app:status');
			invalidate('app:reviews');
		}, 10000);
		return () => clearInterval(interval);
	});
</script>

<svelte:head>
	<title>Revue Dashboard — AI Code Review</title>
</svelte:head>

<!-- Metrics Overview (KPI Row) -->
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
		{#if data.statusOk}
			<span class="metric-value live-pill">
				<span class="dot"></span>
				Live Active
			</span>
		{:else}
			<span class="metric-value offline-pill">
				<span class="dot offline"></span>
				Offline / Stale
			</span>
		{/if}
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

			<button class="trigger-btn" onclick={openTriggerModal}>
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
								onclick={() => openReview(review)}
								role="button"
								tabindex="0"
								onkeydown={(e) => {
									if (e.key === 'Enter') openReview(review);
								}}
							>
								<div class="card-top-row">
									{#if review.status === 'failed'}
										<span class="badge error">❌ Review Failed</span>
									{:else if review.status === 'running'}
										<span class="badge running">⏳ Analyzing...</span>
									{:else if review.status === 'pending'}
										<span class="badge pending">⏳ Pending...</span>
									{:else}
										<span class="priority-pill p2">● P2 Medium</span>
									{/if}
									<span class="forge-pill {review.forge}">{review.forge}</span>
									{#if review.lines_added !== undefined && review.lines_removed !== undefined}
										<span class="diff-chip">
											+{review.lines_added} −{review.lines_removed}
										</span>
									{/if}
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
								onclick={() => openReview(review)}
								role="button"
								tabindex="0"
								onkeydown={(e) => {
									if (e.key === 'Enter') openReview(review);
								}}
							>
								<div class="card-top-row">
									{#if review.status === 'running'}
										<span class="badge running">⏳ Analyzing (Pass 1 & 2)...</span>
									{:else if review.status === 'pending'}
										<span class="badge pending">⏳ Pending...</span>
									{:else if review.status === 'failed'}
										<span class="badge error">❌ Review Failed</span>
									{:else}
										<span class="badge clean">✔ Approved Clean</span>
									{/if}
									<span class="forge-pill {review.forge}">{review.forge}</span>
									{#if review.lines_added !== undefined && review.lines_removed !== undefined}
										<span class="diff-chip">+{review.lines_added} −{review.lines_removed}</span>
									{/if}
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
							<tr onclick={() => openReview(review)}>
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
									{#if review.status === 'running'}
										<span class="badge running">Analyzing</span>
									{:else if review.status === 'pending'}
										<span class="badge pending">Pending</span>
									{:else if review.status === 'failed'}
										<span class="badge error">Failed</span>
									{:else if (review.findings ?? []).length === 0}
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
											openReview(review);
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

<style>
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
		font-size: 0.85rem;
		font-weight: 500;
		color: var(--text-muted);
	}

	.metric-value {
		font-size: 1.75rem;
		font-weight: 700;
		letter-spacing: -0.02em;
		color: #f8fafc;
	}

	.metric-value.live-pill {
		display: inline-flex;
		align-items: center;
		gap: 0.6rem;
		font-size: 1.35rem;
		color: #34d399;
	}

	.metric-value.offline-pill {
		display: inline-flex;
		align-items: center;
		gap: 0.6rem;
		font-size: 1.25rem;
		color: #f87171;
	}

	.dot {
		width: 10px;
		height: 10px;
		border-radius: 50%;
		background: #10b981;
		box-shadow: 0 0 12px #10b981;
		animation: pulseDot 2s ease-in-out infinite;
	}

	.dot.offline {
		background: #ef4444;
		box-shadow: 0 0 12px #ef4444;
		animation: none;
	}

	@keyframes pulseDot {
		0%, 100% { opacity: 1; transform: scale(1); }
		50% { opacity: 0.4; transform: scale(0.85); }
	}

	.metric-sub {
		font-size: 0.75rem;
		color: #64748b;
	}

	.section {
		display: flex;
		flex-direction: column;
		gap: 1.25rem;
	}

	.section-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 1rem;
	}

	.section-header h2 {
		font-size: 1.25rem;
		font-weight: 600;
		margin: 0;
		letter-spacing: -0.01em;
	}

	.section-header .sub {
		font-size: 0.85rem;
		color: var(--text-muted);
	}

	.header-actions {
		display: flex;
		align-items: center;
		gap: 0.85rem;
		flex-wrap: wrap;
	}

	.view-toggle {
		display: flex;
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
		padding: 0.2rem;
		gap: 0.2rem;
	}

	.toggle-btn {
		background: transparent;
		border: none;
		color: #8b949e;
		font-size: 0.8rem;
		font-weight: 600;
		padding: 0.35rem 0.65rem;
		border-radius: 0.35rem;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 0.35rem;
		transition: all 0.15s ease;
	}

	.toggle-btn:hover {
		color: #f0f6fc;
	}

	.toggle-btn.active {
		background: #21262d;
		color: #f97316;
		box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
	}

	.search-input {
		background: #161b22;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
		padding: 0.45rem 0.85rem;
		color: var(--text);
		font-size: 0.85rem;
		outline: none;
		min-width: 220px;
		transition: border-color 0.2s ease;
	}

	.search-input:focus {
		border-color: #f97316;
	}

	.trigger-btn {
		background: #f97316;
		color: #fff;
		border: none;
		border-radius: 0.5rem;
		padding: 0.5rem 1rem;
		font-size: 0.85rem;
		font-weight: 600;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		transition: background 0.15s ease;
	}

	.trigger-btn:hover {
		background: #ea580c;
	}

	.providers-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: 1.15rem;
	}

	.kanban-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(400px, 1fr));
		gap: 1.5rem;
		align-items: start;
	}

	.kanban-col {
		background: #12171f;
		border: 1px solid rgba(255, 255, 255, 0.05);
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
		padding-bottom: 0.75rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.05);
	}

	.col-title {
		display: flex;
		align-items: center;
		gap: 0.6rem;
	}

	.col-title h3 {
		font-size: 0.95rem;
		font-weight: 600;
		margin: 0;
		letter-spacing: -0.01em;
	}

	.dot-status {
		width: 8px;
		height: 8px;
		border-radius: 50%;
	}

	.dot-status.now {
		background: #f97316;
		box-shadow: 0 0 8px #f97316;
	}

	.dot-status.next {
		background: #3b82f6;
		box-shadow: 0 0 8px #3b82f6;
	}

	.count-badge {
		font-size: 0.75rem;
		font-weight: 700;
		background: rgba(249, 115, 22, 0.15);
		color: #f97316;
		padding: 0.15rem 0.55rem;
		border-radius: 9999px;
	}

	.count-badge.clean {
		background: rgba(59, 130, 246, 0.15);
		color: #60a5fa;
	}

	.cards-col {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.empty-column {
		text-align: center;
		padding: 2.5rem 1rem;
		color: #64748b;
		font-size: 0.85rem;
		border: 1px dashed rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
	}

	.triage-card {
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.65rem;
		padding: 1.1rem;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		cursor: pointer;
		transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
		text-align: left;
	}

	.triage-card:hover {
		border-color: rgba(249, 115, 22, 0.4);
		transform: translateY(-2px);
		box-shadow: 0 6px 16px rgba(0, 0, 0, 0.4);
	}

	.triage-card.clean-card:hover {
		border-color: rgba(59, 130, 246, 0.4);
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
		padding: 0.15rem 0.5rem;
		border-radius: 0.3rem;
		letter-spacing: 0.02em;
	}

	.priority-pill.p2 {
		background: rgba(245, 158, 11, 0.15);
		color: #fbbf24;
	}

	.forge-pill {
		font-size: 0.65rem;
		font-weight: 700;
		padding: 0.15rem 0.45rem;
		border-radius: 0.25rem;
		text-transform: uppercase;
		background: #21262d;
		color: #8b949e;
	}

	.forge-pill.forgejo {
		background: rgba(16, 185, 129, 0.15);
		color: #34d399;
	}

	.diff-chip {
		font-size: 0.7rem;
		font-family: monospace;
		color: #8b949e;
		background: rgba(255, 255, 255, 0.04);
		padding: 0.1rem 0.4rem;
		border-radius: 0.25rem;
		margin-left: auto;
	}

	.card-pr-name {
		font-size: 0.85rem;
		font-weight: 600;
		color: #f97316;
		letter-spacing: -0.01em;
	}

	.clean-card .card-pr-name {
		color: #60a5fa;
	}

	.card-pr-title {
		font-size: 0.95rem;
		font-weight: 600;
		color: #f0f6fc;
		line-height: 1.35;
		margin-top: -0.25rem;
	}

	.ai-summary-callout {
		background: rgba(249, 115, 22, 0.05);
		border-left: 2px solid #f97316;
		padding: 0.6rem 0.8rem;
		border-radius: 0 0.35rem 0.35rem 0;
		display: flex;
		gap: 0.5rem;
		align-items: flex-start;
	}

	.ai-summary-callout.clean {
		background: rgba(59, 130, 246, 0.05);
		border-left-color: #3b82f6;
	}

	.callout-icon {
		font-size: 0.85rem;
		line-height: 1.2;
	}

	.ai-summary-callout p {
		margin: 0;
		font-size: 0.775rem;
		color: #cbd5e1;
		line-height: 1.45;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}

	.card-footer-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-top: 0.25rem;
		padding-top: 0.5rem;
		border-top: 1px solid rgba(255, 255, 255, 0.05);
	}

	.author-tag {
		font-size: 0.725rem;
		color: #64748b;
	}

	.card-action-btn {
		font-size: 0.75rem;
		font-weight: 600;
		color: #f97316;
		display: flex;
		align-items: center;
		gap: 0.25rem;
	}

	.clean-card .card-action-btn {
		color: #60a5fa;
	}

	/* Table View */
	.reviews-table-wrap {
		background: #161b22;
		border: 1px solid var(--border);
		border-radius: 0.75rem;
		overflow: hidden;
	}

	.reviews-table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.85rem;
		text-align: left;
	}

	.reviews-table th {
		background: #0f131a;
		padding: 0.85rem 1.15rem;
		color: var(--text-muted);
		font-weight: 600;
		border-bottom: 1px solid var(--border);
	}

	.reviews-table td {
		padding: 1rem 1.15rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.04);
		color: var(--text);
	}

	.reviews-table tr {
		cursor: pointer;
		transition: background 0.15s ease;
	}

	.reviews-table tr:hover {
		background: rgba(255, 255, 255, 0.02);
	}

	.pr-cell {
		display: flex;
		flex-direction: column;
		gap: 0.2rem;
	}

	.repo-pr {
		font-weight: 600;
		color: #f97316;
	}

	.title-preview {
		font-size: 0.775rem;
		color: var(--text-muted);
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

	.badge.pending {
		background: rgba(148, 163, 184, 0.15);
		color: #94a3b8;
		border: 1px solid rgba(148, 163, 184, 0.3);
	}

	.badge.error {
		background: rgba(239, 68, 68, 0.15);
		color: #f87171;
		border: 1px solid rgba(239, 68, 68, 0.3);
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
