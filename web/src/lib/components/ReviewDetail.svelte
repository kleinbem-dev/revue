<script lang="ts">
	import type { ReviewRecord } from '../types';

	let {
		review,
		onClose
	}: {
		review: ReviewRecord;
		onClose: () => void;
	} = $props();

	let filter = $state<'all' | 'critical' | 'error' | 'warning'>('all');

	const filteredFindings = $derived(
		filter === 'all'
			? review.findings
			: review.findings.filter((f) => f.severity === filter)
	);
</script>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape') onClose(); }} />

<div class="overlay" onclick={onClose} role="presentation">
	<div
		class="modal"
		onclick={(e) => e.stopPropagation()}
		onkeydown={(e) => e.stopPropagation()}
		role="dialog"
		aria-modal="true"
		tabindex="-1"
	>
		<div class="modal-header">
			<div class="title-meta">
				<span class="forge-tag {review.forge}">{review.forge}</span>
				<h2>{review.repo} #{review.pr_number}</h2>
				{#if review.title}
					<span class="pr-title">{review.title}</span>
				{/if}
			</div>
			<button class="close-btn" onclick={onClose} aria-label="Close">✕</button>
		</div>

		<div class="modal-stats">
			<div class="stat-pill">
				<span class="label">Pass 1 Candidates:</span>
				<span class="val">{review.pass1_candidates}</span>
			</div>
			<div class="stat-pill">
				<span class="label">Pass 2 Verified:</span>
				<span class="val success">{review.pass2_verified}</span>
			</div>
			<div class="stat-pill">
				<span class="label">Engine:</span>
				<span class="val">{review.provider_used}</span>
			</div>
			<div class="stat-pill">
				<span class="label">Duration:</span>
				<span class="val">{review.duration_secs}s</span>
			</div>
		</div>

		<div class="modal-body">
			<section class="summary-section">
				<h3>Review Summary</h3>
				<div class="summary-content">
					<p>{review.summary}</p>
				</div>
			</section>

			<section class="findings-section">
				<div class="findings-header">
					<h3>Findings ({filteredFindings.length})</h3>
					<div class="filter-pills">
						<button
							class="pill"
							class:active={filter === 'all'}
							onclick={() => (filter = 'all')}
						>
							All ({review.findings.length})
						</button>
						<button
							class="pill error"
							class:active={filter === 'error'}
							onclick={() => (filter = 'error')}
						>
							Errors
						</button>
						<button
							class="pill warning"
							class:active={filter === 'warning'}
							onclick={() => (filter = 'warning')}
						>
							Warnings
						</button>
					</div>
				</div>

				<div class="findings-list">
					{#if filteredFindings.length === 0}
						<div class="empty-state">No findings matching this filter. Clean diff! ✨</div>
					{:else}
						{#each filteredFindings as finding}
							<div class="finding-card {finding.severity}">
								<div class="finding-top">
									<span class="severity-badge {finding.severity}">
										{finding.severity.toUpperCase()}
									</span>
									<span class="location">
										<code>{finding.file}:{finding.line}</code>
									</span>
									{#if finding.confidence}
										<span class="confidence">
											Judge Confidence: {finding.confidence}/10
										</span>
									{/if}
								</div>
								<h4>{finding.title}</h4>
								<p class="finding-body">{finding.body}</p>
								{#if finding.snippet}
									<pre class="snippet"><code>{finding.snippet}</code></pre>
								{/if}
							</div>
						{/each}
					{/if}
				</div>
			</section>
		</div>
	</div>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.7);
		backdrop-filter: blur(6px);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 100;
		padding: 1.5rem;
	}

	.modal {
		background: #0f172a;
		border: 1px solid #1e293b;
		border-radius: 1rem;
		max-width: 860px;
		width: 100%;
		max-height: 90vh;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.5);
	}

	.modal-header {
		padding: 1.25rem 1.75rem;
		border-bottom: 1px solid #1e293b;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.title-meta {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		flex-wrap: wrap;
	}

	h2 {
		font-size: 1.25rem;
		font-weight: 700;
		color: #f8fafc;
		margin: 0;
	}

	.pr-title {
		color: #94a3b8;
		font-size: 0.95rem;
	}

	.forge-tag {
		font-size: 0.7rem;
		font-weight: 600;
		padding: 0.15rem 0.5rem;
		border-radius: 9999px;
		text-transform: uppercase;
		background: #1e293b;
		color: #94a3b8;
	}

	.forge-tag.forgejo {
		background: rgba(16, 185, 129, 0.15);
		color: #34d399;
	}

	.close-btn {
		background: none;
		border: none;
		color: #64748b;
		font-size: 1.25rem;
		cursor: pointer;
		padding: 0.25rem;
		border-radius: 0.35rem;
	}

	.close-btn:hover {
		color: #f1f5f9;
		background: #1e293b;
	}

	.modal-stats {
		display: flex;
		gap: 0.85rem;
		padding: 0.85rem 1.75rem;
		background: #090e17;
		border-bottom: 1px solid #1e293b;
		overflow-x: auto;
	}

	.stat-pill {
		display: flex;
		gap: 0.4rem;
		font-size: 0.8rem;
	}

	.stat-pill .label {
		color: #64748b;
	}

	.stat-pill .val {
		color: #cbd5e1;
		font-weight: 600;
	}

	.stat-pill .val.success {
		color: #34d399;
	}

	.modal-body {
		padding: 1.75rem;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 1.5rem;
	}

	h3 {
		font-size: 1rem;
		font-weight: 600;
		color: #e2e8f0;
		margin: 0 0 0.75rem 0;
	}

	.summary-content {
		background: rgba(30, 41, 59, 0.4);
		border: 1px solid #1e293b;
		border-radius: 0.65rem;
		padding: 1rem;
		color: #cbd5e1;
		font-size: 0.9rem;
		line-height: 1.6;
	}

	.findings-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 1rem;
	}

	.filter-pills {
		display: flex;
		gap: 0.5rem;
	}

	.pill {
		background: #1e293b;
		border: 1px solid #334155;
		color: #94a3b8;
		padding: 0.25rem 0.65rem;
		border-radius: 9999px;
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.pill:hover,
	.pill.active {
		background: #334155;
		color: #f8fafc;
	}

	.findings-list {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.finding-card {
		border: 1px solid #1e293b;
		background: rgba(15, 23, 42, 0.6);
		border-radius: 0.65rem;
		padding: 1.15rem;
		display: flex;
		flex-direction: column;
		gap: 0.65rem;
	}

	.finding-card.error {
		border-left: 3px solid #ef4444;
	}

	.finding-card.warning {
		border-left: 3px solid #f59e0b;
	}

	.finding-top {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.severity-badge {
		font-size: 0.65rem;
		font-weight: 700;
		padding: 0.15rem 0.45rem;
		border-radius: 0.25rem;
	}

	.severity-badge.error {
		background: rgba(239, 68, 68, 0.15);
		color: #f87171;
	}

	.severity-badge.warning {
		background: rgba(245, 158, 11, 0.15);
		color: #fbbf24;
	}

	.location code {
		font-size: 0.8rem;
		color: #93c5fd;
		background: rgba(30, 41, 59, 0.7);
		padding: 0.15rem 0.45rem;
		border-radius: 0.25rem;
	}

	.confidence {
		font-size: 0.75rem;
		color: #64748b;
		margin-left: auto;
	}

	h4 {
		font-size: 0.95rem;
		font-weight: 600;
		color: #f1f5f9;
		margin: 0;
	}

	.finding-body {
		font-size: 0.85rem;
		color: #94a3b8;
		margin: 0;
		line-height: 1.5;
	}

	.snippet {
		background: #020617;
		border: 1px solid #1e293b;
		border-radius: 0.45rem;
		padding: 0.75rem;
		font-family: ui-monospace, monospace;
		font-size: 0.8rem;
		color: #cbd5e1;
		overflow-x: auto;
		margin: 0;
	}

	.empty-state {
		padding: 2rem;
		text-align: center;
		color: #64748b;
		font-size: 0.9rem;
	}
</style>
