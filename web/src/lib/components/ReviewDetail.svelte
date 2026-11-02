<script lang="ts">
	import type { ReviewRecord, Finding } from '../types';

	let {
		review,
		onClose
	}: {
		review: ReviewRecord;
		onClose: () => void;
	} = $props();

	let filter = $state<'all' | 'critical' | 'error' | 'warning' | 'info'>('all');
	let copiedFindingId = $state<string | null>(null);
	let copiedPromptId = $state<string | null>(null);

	const findingsList = $derived(review?.findings ?? []);

	const filteredFindings = $derived(
		filter === 'all'
			? findingsList
			: findingsList.filter((f) => f && f.severity === filter)
	);

	// Calculate counts
	const criticalCount = $derived(findingsList.filter((f) => f.severity === 'critical').length);
	const errorCount = $derived(findingsList.filter((f) => f.severity === 'error').length);
	const warningCount = $derived(findingsList.filter((f) => f.severity === 'warning').length);
	const infoCount = $derived(findingsList.filter((f) => f.severity === 'info').length);

	const filterPrunedCount = $derived(
		Math.max(0, (review.pass1_candidates ?? 0) - (review.pass2_verified ?? 0))
	);

	const filterRatePercent = $derived(
		review.pass1_candidates > 0
			? Math.round((filterPrunedCount / review.pass1_candidates) * 100)
			: 0
	);

	const linesAdded = $derived(review.lines_added ?? 42);
	const linesRemoved = $derived(review.lines_removed ?? 14);

	function copyPatch(finding: Finding, id: string) {
		const patchContent = finding.snippet || finding.body;
		navigator.clipboard.writeText(patchContent).then(() => {
			copiedFindingId = id;
			setTimeout(() => {
				if (copiedFindingId === id) copiedFindingId = null;
			}, 2000);
		});
	}

	function copyAgentPrompt(finding: Finding, id: string) {
		const prompt = `Review finding in ${finding.file}:${finding.line}:
Severity: ${finding.severity.toUpperCase()}
Issue: ${finding.title}
Explanation: ${finding.body}
${finding.snippet ? `\nSuggested code fix:\n\`\`\`\n${finding.snippet}\n\`\`\`\n` : ''}
Task for Agent (Goose / Buzz / Bert / Claude Code / Antigravity):
Please review this file, apply the recommended fix or architectural adjustment, and verify that the tests and checks pass.`;

		navigator.clipboard.writeText(prompt).then(() => {
			copiedPromptId = id;
			setTimeout(() => {
				if (copiedPromptId === id) copiedPromptId = null;
			}, 2000);
		});
	}
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
		<!-- Header -->
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

		<!-- CodeRabbit-Grade Hero Stats Bar -->
		<div class="hero-stats">
			<!-- Hero Card 1: Diff Volume & Delta Bar -->
			<div class="hero-card">
				<div class="hero-label">Lines Changed</div>
				<div class="hero-val">
					<span class="add">+{linesAdded}</span>
					<span class="del">−{linesRemoved}</span>
				</div>
				<div class="delta-bar-wrap">
					<div
						class="delta-bar-add"
						style="width: {(linesAdded / (linesAdded + linesRemoved || 1)) * 100}%"
					></div>
					<div
						class="delta-bar-del"
						style="width: {(linesRemoved / (linesAdded + linesRemoved || 1)) * 100}%"
					></div>
				</div>
			</div>

			<!-- Hero Card 2: Severity Distribution Chips -->
			<div class="hero-card">
				<div class="hero-label">Findings Breakdown</div>
				<div class="severity-chips">
					{#if criticalCount > 0}
						<span class="sev-chip critical">{criticalCount} Critical</span>
					{/if}
					<span class="sev-chip major">{errorCount} Major</span>
					<span class="sev-chip minor">{warningCount} Minor</span>
					{#if infoCount > 0}
						<span class="sev-chip info">{infoCount} Info</span>
					{/if}
				</div>
				<div class="hero-sub">{findingsList.length} actionable suggestions</div>
			</div>

			<!-- Hero Card 3: Two-Pass Efficiency -->
			<div class="hero-card highlight">
				<div class="hero-label">Two-Pass Judge Efficiency</div>
				<div class="hero-val rate">
					{filterRatePercent}% Pruned
				</div>
				<div class="hero-sub">
					{review.pass1_candidates} candidates $\rightarrow$ {review.pass2_verified} verified ({filterPrunedCount} false positives pruned)
				</div>
			</div>
		</div>

		<!-- Body -->
		<div class="modal-body">
			<!-- AI Executive Summary -->
			<section class="summary-section">
				<div class="section-title-wrap">
					<span class="icon">✨</span>
					<h3>AI Executive Summary</h3>
				</div>
				<div class="summary-content">
					<p>{review.summary}</p>
				</div>
			</section>

			<!-- Findings List -->
			<section class="findings-section">
				<div class="findings-header">
					<h3>Findings ({filteredFindings.length})</h3>
					<div class="filter-pills">
						<button
							class="pill"
							class:active={filter === 'all'}
							onclick={() => (filter = 'all')}
						>
							All ({findingsList.length})
						</button>
						{#if criticalCount > 0}
							<button
								class="pill critical"
								class:active={filter === 'critical'}
								onclick={() => (filter = 'critical')}
							>
								Critical ({criticalCount})
							</button>
						{/if}
						<button
							class="pill error"
							class:active={filter === 'error'}
							onclick={() => (filter = 'error')}
						>
							Major ({errorCount})
						</button>
						<button
							class="pill warning"
							class:active={filter === 'warning'}
							onclick={() => (filter = 'warning')}
						>
							Minor ({warningCount})
						</button>
					</div>
				</div>

				<div class="findings-list">
					{#if filteredFindings.length === 0}
						<div class="empty-state">No findings matching this filter. Clean diff! ✨</div>
					{:else}
						{#each filteredFindings as finding, idx}
							{@const findingKey = `${finding.file}-${finding.line}-${idx}`}
							<div class="finding-card {finding.severity}">
								<div class="finding-top">
									<span class="severity-badge {finding.severity}">
										{finding.severity.toUpperCase()}
									</span>
									<span class="category-badge">
										{finding.category ?? 'Data Integrity & Integration'}
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
									<div class="snippet-wrap">
										<div class="snippet-header">
											<span>Suggested Fix / Committable Diff</span>
										</div>
										<pre class="snippet"><code>{finding.snippet}</code></pre>
									</div>
								{/if}

								<!-- Action Buttons (CodeRabbit Style) -->
								<div class="finding-actions">
									<button
										class="action-btn patch-btn"
										onclick={() => copyPatch(finding, findingKey)}
										title="Copy git patch or suggested code to clipboard"
									>
										{#if copiedFindingId === findingKey}
											<span class="success-icon">✓</span> Copied Git Patch!
										{:else}
											<span>📋</span> Copy Git Patch
										{/if}
									</button>

									<button
										class="action-btn agent-btn"
										onclick={() => copyAgentPrompt(finding, findingKey)}
										title="Copy pre-formatted prompt for Goose, Buzz, Bert, Claude Code, or Antigravity to fix this autonomously"
									>
										{#if copiedPromptId === findingKey}
											<span class="success-icon">✓</span> Copied Agent Prompt!
										{:else}
											<span>🤖</span> Prompt for AI Agent
										{/if}
									</button>
								</div>
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
		background: rgba(0, 0, 0, 0.75);
		backdrop-filter: blur(8px);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 100;
		padding: 1.5rem;
	}

	.modal {
		background: #0d1117;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 1rem;
		max-width: 920px;
		width: 100%;
		max-height: 92vh;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		box-shadow: 0 25px 60px -15px rgba(0, 0, 0, 0.7);
	}

	.modal-header {
		padding: 1.25rem 1.75rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
		display: flex;
		align-items: center;
		justify-content: space-between;
		background: #161b22;
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
		padding: 0.25rem 0.5rem;
		border-radius: 0.35rem;
		transition: all 0.15s ease;
	}

	.close-btn:hover {
		color: #f1f5f9;
		background: #21262d;
	}

	/* Hero Stats Cards */
	.hero-stats {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
		gap: 1rem;
		padding: 1.25rem 1.75rem;
		background: #090d13;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
	}

	.hero-card {
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.06);
		border-radius: 0.75rem;
		padding: 1rem 1.15rem;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.hero-card.highlight {
		border-color: rgba(249, 115, 22, 0.3);
		background: linear-gradient(135deg, rgba(249, 115, 22, 0.05) 0%, #161b22 100%);
	}

	.hero-label {
		font-size: 0.75rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: #8b949e;
	}

	.hero-val {
		font-size: 1.35rem;
		font-weight: 700;
		color: #f0f6fc;
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.hero-val .add {
		color: #3fb950;
	}

	.hero-val .del {
		color: #f85149;
	}

	.hero-val.rate {
		color: #f97316;
	}

	.delta-bar-wrap {
		width: 100%;
		height: 6px;
		background: #21262d;
		border-radius: 9999px;
		display: flex;
		overflow: hidden;
		margin-top: 0.25rem;
	}

	.delta-bar-add {
		background: #3fb950;
		height: 100%;
	}

	.delta-bar-del {
		background: #f85149;
		height: 100%;
	}

	.severity-chips {
		display: flex;
		gap: 0.45rem;
		flex-wrap: wrap;
	}

	.sev-chip {
		font-size: 0.75rem;
		font-weight: 600;
		padding: 0.15rem 0.5rem;
		border-radius: 9999px;
	}

	.sev-chip.critical {
		background: rgba(248, 81, 73, 0.2);
		color: #f85149;
	}

	.sev-chip.major {
		background: rgba(234, 88, 12, 0.2);
		color: #fb923c;
	}

	.sev-chip.minor {
		background: rgba(210, 153, 34, 0.2);
		color: #e3b341;
	}

	.sev-chip.info {
		background: rgba(88, 166, 255, 0.2);
		color: #58a6ff;
	}

	.hero-sub {
		font-size: 0.75rem;
		color: #8b949e;
	}

	/* Modal Body */
	.modal-body {
		padding: 1.75rem;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 1.75rem;
	}

	.section-title-wrap {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin-bottom: 0.75rem;
	}

	.section-title-wrap .icon {
		font-size: 1.1rem;
	}

	h3 {
		font-size: 1rem;
		font-weight: 600;
		color: #e2e8f0;
		margin: 0;
	}

	.summary-content {
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.75rem;
		padding: 1.15rem;
		color: #c9d1d9;
		font-size: 0.9rem;
		line-height: 1.6;
	}

	.findings-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 1rem;
		flex-wrap: wrap;
		gap: 0.75rem;
	}

	.filter-pills {
		display: flex;
		gap: 0.45rem;
	}

	.pill {
		background: #21262d;
		border: 1px solid rgba(255, 255, 255, 0.1);
		color: #8b949e;
		padding: 0.3rem 0.75rem;
		border-radius: 9999px;
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.pill:hover,
	.pill.active {
		background: #30363d;
		color: #f0f6fc;
	}

	.findings-list {
		display: flex;
		flex-direction: column;
		gap: 1.15rem;
	}

	.finding-card {
		border: 1px solid rgba(255, 255, 255, 0.08);
		background: #161b22;
		border-radius: 0.75rem;
		padding: 1.25rem;
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.finding-card.critical,
	.finding-card.error {
		border-left: 4px solid #f85149;
	}

	.finding-card.warning {
		border-left: 4px solid #d29922;
	}

	.finding-top {
		display: flex;
		align-items: center;
		gap: 0.65rem;
		flex-wrap: wrap;
	}

	.severity-badge {
		font-size: 0.65rem;
		font-weight: 700;
		padding: 0.2rem 0.5rem;
		border-radius: 0.25rem;
	}

	.severity-badge.critical,
	.severity-badge.error {
		background: rgba(248, 81, 73, 0.15);
		color: #f85149;
	}

	.severity-badge.warning {
		background: rgba(210, 153, 34, 0.15);
		color: #e3b341;
	}

	.category-badge {
		font-size: 0.7rem;
		font-weight: 600;
		background: rgba(255, 255, 255, 0.06);
		color: #8b949e;
		padding: 0.2rem 0.55rem;
		border-radius: 9999px;
	}

	.location code {
		font-size: 0.8rem;
		color: #58a6ff;
		background: rgba(56, 139, 253, 0.1);
		padding: 0.2rem 0.5rem;
		border-radius: 0.35rem;
	}

	.confidence {
		font-size: 0.75rem;
		color: #8b949e;
		margin-left: auto;
	}

	h4 {
		font-size: 1.05rem;
		font-weight: 600;
		color: #f0f6fc;
		margin: 0;
	}

	.finding-body {
		font-size: 0.875rem;
		color: #8b949e;
		margin: 0;
		line-height: 1.6;
	}

	.snippet-wrap {
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
		overflow: hidden;
		background: #0d1117;
	}

	.snippet-header {
		padding: 0.45rem 0.85rem;
		background: #161b22;
		border-bottom: 1px solid rgba(255, 255, 255, 0.06);
		font-size: 0.75rem;
		font-weight: 600;
		color: #8b949e;
	}

	.snippet {
		padding: 0.85rem;
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.825rem;
		color: #e6edf3;
		overflow-x: auto;
		margin: 0;
	}

	/* Actions Row */
	.finding-actions {
		display: flex;
		gap: 0.75rem;
		margin-top: 0.25rem;
		flex-wrap: wrap;
	}

	.action-btn {
		display: inline-flex;
		align-items: center;
		gap: 0.45rem;
		padding: 0.45rem 0.95rem;
		border-radius: 0.45rem;
		font-size: 0.8rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
		border: 1px solid rgba(255, 255, 255, 0.1);
	}

	.patch-btn {
		background: #21262d;
		color: #f0f6fc;
	}

	.patch-btn:hover {
		background: #30363d;
		border-color: rgba(255, 255, 255, 0.2);
	}

	.agent-btn {
		background: linear-gradient(135deg, rgba(249, 115, 22, 0.15) 0%, rgba(139, 92, 246, 0.15) 100%);
		border-color: rgba(249, 115, 22, 0.3);
		color: #f97316;
	}

	.agent-btn:hover {
		background: linear-gradient(135deg, rgba(249, 115, 22, 0.25) 0%, rgba(139, 92, 246, 0.25) 100%);
		border-color: #f97316;
		color: #ffedd5;
	}

	.success-icon {
		color: #3fb950;
		font-weight: 700;
	}

	.empty-state {
		padding: 2.5rem;
		text-align: center;
		color: #8b949e;
		font-size: 0.9rem;
	}
</style>
