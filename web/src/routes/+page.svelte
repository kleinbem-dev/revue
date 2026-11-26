<script lang="ts">
	import ProviderCard from '#lib/components/ProviderCard.svelte';
	import {
		computeNoiseReductionPct,
		type ReviewRecord,
		type ProviderStatus,
		type DaemonMetrics
	} from '#lib';
	import type { PageData } from './$types';
	import { pushState, invalidate } from '$app/navigation';
	import { onMount } from 'svelte';

	let { data }: { data: PageData } = $props();

	let layoutMode = $state<'kanban' | 'table'>('kanban');
	let searchQuery = $state('');

	const reviews = $derived<ReviewRecord[]>(data.reviews ?? []);
	const metrics = $derived<DaemonMetrics>(data.metrics);
	const providers = $derived<ProviderStatus[]>(data.providers ?? []);

	const filterEfficiencyPct = $derived(
		computeNoiseReductionPct(metrics.pass1_candidates, metrics.verified_findings)
	);

	const filteredReviews = $derived(
		reviews.filter(
			(r: ReviewRecord) =>
				(r?.repo ?? '').toLowerCase().includes((searchQuery ?? '').toLowerCase()) ||
				String(r?.pr_number ?? '').includes(searchQuery ?? '')
		)
	);

	const nowReviews = $derived(
		filteredReviews.filter(
			(r: ReviewRecord) => (r.findings ?? []).length > 0 || r.triage_status === 'now'
		)
	);

	const nextReviews = $derived(
		filteredReviews.filter(
			(r: ReviewRecord) => (r.findings ?? []).length === 0 && r.triage_status !== 'now'
		)
	);

	function openReview(review: ReviewRecord) {
		pushState(`?review=${review.id}`, { selectedReviewId: review.id });
	}

	function handleActivateKeydown(e: KeyboardEvent, review: ReviewRecord) {
		if (e.key === 'Enter' || e.key === ' ') {
			e.preventDefault();
			openReview(review);
		}
	}

	function priorityMeta(risk?: ReviewRecord['risk_level']): { cls: string; label: string } {
		if (risk === 'high') return { cls: 'p1', label: '● P1 High' };
		if (risk === 'medium') return { cls: 'p2', label: '● P2 Medium' };
		return { cls: 'p3', label: '● P3 Low' };
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

<section class="metrics-grid">
	<div class="metric-card">
		<span class="metric-label">Estimated SaaS Savings</span>
		<span class="metric-value">{metrics.avg_cost_saved}</span>
		<span class="metric-sub">vs $24–$30/seat/mo commercial tools</span>
	</div>

	<div class="metric-card">
		<span class="metric-label">Two-Pass Filter Efficiency</span>
		<span class="metric-value">
			{filterEfficiencyPct !== null ? `${filterEfficiencyPct}%` : '—'}
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

<section class="section">
	<div class="section-header">
		<div>
			<h2>Multi-Provider Cascade Status</h2>
			<span class="sub">Failover priority: Primary → Fallback 1 → Fallback 2</span>
		</div>
	</div>

	<div class="providers-grid">
		{#each providers as provider (provider.id ?? provider.name)}
			<ProviderCard {provider} />
		{/each}
	</div>
</section>

<section class="section">
	<div class="section-header">
		<div>
			<h2>Code Review Feed</h2>
			<span class="sub">Two-Pass Detective & Judge execution history</span>
		</div>

		<div class="header-actions">
			<div class="view-toggle" role="group" aria-label="Layout mode">
				<button
					type="button"
					class="toggle-btn"
					class:active={layoutMode === 'kanban'}
					onclick={() => (layoutMode = 'kanban')}
					title="Kanban Triage View"
				>
					<span>📋</span> Kanban
				</button>
				<button
					type="button"
					class="toggle-btn"
					class:active={layoutMode === 'table'}
					onclick={() => (layoutMode = 'table')}
					title="Compact Table View"
				>
					<span>☰</span> Table
				</button>
			</div>

			<input
				type="search"
				placeholder="Search repo, PR #..."
				aria-label="Search reviews by repository or PR number"
				class="search-input"
				bind:value={searchQuery}
			/>

			<button type="button" class="trigger-btn" onclick={openTriggerModal}>
				+ Trigger Review
			</button>
		</div>
	</div>

	{#if layoutMode === 'kanban'}
		<div class="kanban-grid">
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
						{#each nowReviews as review (review.id)}
							{@const prio = priorityMeta(review.risk_level)}
							<div
								class="triage-card"
								onclick={() => openReview(review)}
								role="button"
								tabindex="0"
								onkeydown={(e) => handleActivateKeydown(e, review)}
							>
								<div class="card-top-row">
									{#if review.status === 'failed'}
										<span class="badge error">❌ Review Failed</span>
									{:else if review.status === 'running'}
										<span class="badge running">⏳ Analyzing...</span>
									{:else if review.status === 'pending'}
										<span class="badge pending">⏳ Pending...</span>
									{:else}
										<span class="priority-pill {prio.cls}">{prio.label}</span>
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

								<div class="ai-summary-callout">
									<span class="callout-icon">✨</span>
									<p>{review.summary}</p>
								</div>

								<div class="card-footer-row">
									<span class="author-tag">@{review.author ?? 'unknown'} · {review.created_at}</span>
									<div class="card-action-btn">Inspect & Fix →</div>
								</div>
							</div>
						{/each}
					{/if}
				</div>
			</div>

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
						{#each nextReviews as review (review.id)}
							<div
								class="triage-card clean-card"
								onclick={() => openReview(review)}
								role="button"
								tabindex="0"
								onkeydown={(e) => handleActivateKeydown(e, review)}
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
									<span class="author-tag">@{review.author ?? 'unknown'} · {review.created_at}</span>
									<div class="card-action-btn">View Details →</div>
								</div>
							</div>
						{/each}
					{/if}
				</div>
			</div>
		</div>
	{:else}
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
						{#each filteredReviews as review (review.id)}
							<tr
								tabindex="0"
								onclick={() => openReview(review)}
								onkeydown={(e) => handleActivateKeydown(e, review)}
							>
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
										type="button"
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
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.85rem;
		padding: 1.25rem 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
		box-shadow: var(--shadow-elevation-1);
	}

	.metric-card.highlight {
		border-color: var(--color-accent);
		background: linear-gradient(135deg, var(--color-accent-soft) 0%, var(--color-surface) 100%);
	}

	.metric-label {
		font-size: 0.85rem;
		font-weight: 500;
		color: var(--color-muted);
	}

	.metric-value {
		font-size: 1.75rem;
		font-weight: 700;
		letter-spacing: -0.02em;
		color: var(--color-heading);
	}

	.metric-value.live-pill {
		display: inline-flex;
		align-items: center;
		gap: 0.6rem;
		font-size: 1.35rem;
		color: var(--color-status-success);
	}

	.metric-value.offline-pill {
		display: inline-flex;
		align-items: center;
		gap: 0.6rem;
		font-size: 1.25rem;
		color: var(--color-status-danger);
	}

	.dot {
		width: 10px;
		height: 10px;
		border-radius: 50%;
		background: var(--color-status-success);
		box-shadow: 0 0 10px var(--color-status-success);
		animation: pulseDot 2s ease-in-out infinite;
	}

	.dot.offline {
		background: var(--color-status-danger);
		box-shadow: none;
		animation: none;
	}

	@keyframes pulseDot {
		0%,
		100% {
			opacity: 1;
			transform: scale(1);
		}
		50% {
			opacity: 0.45;
			transform: scale(0.85);
		}
	}

	.metric-sub {
		font-size: 0.75rem;
		color: var(--color-muted);
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
		color: var(--color-heading);
	}

	.section-header .sub {
		font-size: 0.85rem;
		color: var(--color-muted);
	}

	.header-actions {
		display: flex;
		align-items: center;
		gap: 0.85rem;
		flex-wrap: wrap;
	}

	.view-toggle {
		display: flex;
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.5rem;
		padding: 0.2rem;
		gap: 0.2rem;
	}

	.toggle-btn {
		background: transparent;
		border: none;
		color: var(--color-muted);
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
		color: var(--color-heading);
	}

	.toggle-btn.active {
		background: var(--color-surface-high);
		color: var(--color-accent);
		box-shadow: var(--shadow-elevation-1);
	}

	.search-input {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.5rem;
		padding: 0.45rem 0.85rem;
		color: var(--color-fg);
		font-size: 0.85rem;
		outline: none;
		min-width: 220px;
		transition: border-color 0.2s ease;
	}

	.search-input:focus {
		border-color: var(--color-accent);
	}

	.trigger-btn {
		background: var(--color-accent);
		color: var(--color-accent-fg);
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
		background: var(--color-accent-hover);
	}

	.providers-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: 1.15rem;
	}

	.kanban-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
		gap: 1.5rem;
		align-items: start;
	}

	.kanban-col {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
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
		border-bottom: 1px solid var(--color-line);
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
		color: var(--color-heading);
	}

	.dot-status {
		width: 8px;
		height: 8px;
		border-radius: 50%;
	}

	.dot-status.now {
		background: var(--color-accent);
	}

	.dot-status.next {
		background: var(--color-status-info);
	}

	.count-badge {
		font-size: 0.75rem;
		font-weight: 700;
		background: var(--color-accent-soft);
		color: var(--color-accent-soft-fg);
		padding: 0.15rem 0.55rem;
		border-radius: 9999px;
	}

	.count-badge.clean {
		background: var(--color-status-info-soft);
		color: var(--color-status-info);
	}

	.cards-col {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.empty-column {
		text-align: center;
		padding: 2.5rem 1rem;
		color: var(--color-muted);
		font-size: 0.85rem;
		border: 1px dashed var(--color-outline);
		border-radius: 0.5rem;
	}

	.triage-card {
		background: var(--color-bg);
		border: 1px solid var(--color-line);
		border-radius: 0.65rem;
		padding: 1.1rem;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		cursor: pointer;
		transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
		text-align: left;
	}

	.triage-card:hover,
	.triage-card:focus-visible {
		border-color: var(--color-accent);
		transform: translateY(-2px);
		box-shadow: var(--shadow-elevation-2);
		outline: none;
	}

	.triage-card.clean-card:hover,
	.triage-card.clean-card:focus-visible {
		border-color: var(--color-status-info);
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

	.priority-pill.p1 {
		background: var(--color-status-danger-soft);
		color: var(--color-status-danger);
	}

	.priority-pill.p2 {
		background: var(--color-status-warning-soft);
		color: var(--color-status-warning);
	}

	.priority-pill.p3 {
		background: var(--color-status-info-soft);
		color: var(--color-status-info);
	}

	.forge-pill {
		font-size: 0.65rem;
		font-weight: 700;
		padding: 0.15rem 0.45rem;
		border-radius: 0.25rem;
		text-transform: uppercase;
		background: var(--color-surface-high);
		color: var(--color-muted);
	}

	.forge-pill.forgejo {
		background: var(--color-status-success-soft);
		color: var(--color-status-success);
	}

	.diff-chip {
		font-size: 0.7rem;
		font-family: var(--font-mono);
		color: var(--color-muted);
		background: var(--color-surface-high);
		padding: 0.1rem 0.4rem;
		border-radius: 0.25rem;
		margin-left: auto;
	}

	.card-pr-name {
		font-size: 0.85rem;
		font-weight: 600;
		color: var(--color-accent);
		letter-spacing: -0.01em;
	}

	.clean-card .card-pr-name {
		color: var(--color-status-info);
	}

	.card-pr-title {
		font-size: 0.95rem;
		font-weight: 600;
		color: var(--color-heading);
		line-height: 1.35;
		margin-top: -0.25rem;
	}

	.ai-summary-callout {
		background: var(--color-accent-soft);
		border-left: 2px solid var(--color-accent);
		padding: 0.6rem 0.8rem;
		border-radius: 0 0.35rem 0.35rem 0;
		display: flex;
		gap: 0.5rem;
		align-items: flex-start;
	}

	.ai-summary-callout.clean {
		background: var(--color-status-info-soft);
		border-left-color: var(--color-status-info);
	}

	.callout-icon {
		font-size: 0.85rem;
		line-height: 1.2;
	}

	.ai-summary-callout p {
		margin: 0;
		font-size: 0.775rem;
		color: var(--color-fg);
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
		border-top: 1px solid var(--color-line);
	}

	.author-tag {
		font-size: 0.725rem;
		color: var(--color-muted);
	}

	.card-action-btn {
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--color-accent);
		display: flex;
		align-items: center;
		gap: 0.25rem;
	}

	.clean-card .card-action-btn {
		color: var(--color-status-info);
	}

	.reviews-table-wrap {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.75rem;
		overflow-x: auto;
	}

	.reviews-table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.85rem;
		text-align: left;
	}

	.reviews-table th {
		background: var(--color-surface-high);
		padding: 0.85rem 1.15rem;
		color: var(--color-muted);
		font-weight: 600;
		border-bottom: 1px solid var(--color-line);
	}

	.reviews-table td {
		padding: 1rem 1.15rem;
		border-bottom: 1px solid var(--color-line);
		color: var(--color-fg);
	}

	.reviews-table tbody tr {
		cursor: pointer;
		transition: background 0.15s ease;
	}

	.reviews-table tbody tr:hover,
	.reviews-table tbody tr:focus-visible {
		background: var(--color-surface-high);
		outline: none;
	}

	.pr-cell {
		display: flex;
		flex-direction: column;
		gap: 0.2rem;
	}

	.repo-pr {
		font-weight: 600;
		color: var(--color-accent);
	}

	.title-preview {
		font-size: 0.775rem;
		color: var(--color-muted);
	}

	.badge {
		font-size: 0.75rem;
		font-weight: 600;
		padding: 0.2rem 0.6rem;
		border-radius: 9999px;
	}

	.badge.clean {
		background: var(--color-status-success-soft);
		color: var(--color-status-success);
	}

	.badge.pending {
		background: var(--color-surface-high);
		color: var(--color-muted);
		border: 1px solid var(--color-outline);
	}

	.badge.error {
		background: var(--color-status-danger-soft);
		color: var(--color-status-danger);
		border: 1px solid var(--color-status-danger-border);
	}

	.badge.running {
		background: var(--color-status-warning-soft);
		color: var(--color-status-warning);
		border: 1px solid var(--color-status-warning-border);
		animation: badgePulse 1.5s ease-in-out infinite;
	}

	@keyframes badgePulse {
		0%,
		100% {
			opacity: 1;
			transform: scale(1);
		}
		50% {
			opacity: 0.7;
			transform: scale(0.98);
		}
	}

	.badge.issues {
		background: var(--color-status-warning-soft);
		color: var(--color-status-warning);
	}

	.filter-stat {
		font-size: 0.8rem;
		color: var(--color-muted);
	}

	.view-btn {
		background: var(--color-surface-high);
		border: 1px solid var(--color-outline);
		color: var(--color-heading);
		padding: 0.35rem 0.75rem;
		border-radius: 0.35rem;
		font-size: 0.8rem;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.view-btn:hover {
		border-color: var(--color-accent);
		color: var(--color-accent);
	}

	.empty {
		text-align: center;
		padding: 2.5rem;
		color: var(--color-muted);
	}
</style>
