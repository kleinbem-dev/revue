<script lang="ts">
	import { invalidate } from '$app/navigation';
	import {
		apiFetch,
		computeNoiseReductionPct,
		modalDialog,
		severityBucket,
		type Finding,
		type ReviewRecord,
		type SeverityBucket
	} from '#lib';

	let {
		review,
		onClose
	}: {
		review: ReviewRecord;
		onClose: () => void;
	} = $props();

	let filter = $state<'all' | SeverityBucket>('all');
	let copiedFindingId = $state<string | null>(null);
	let copiedPromptId = $state<string | null>(null);
	let markingFalsePositive = $state<string | null>(null);
	let localDismissed = $state<Record<string, boolean>>({});
	let feedbackError = $state<Record<string, string>>({});
	let synthesizingRule = $state<string | null>(null);
	let synthesizedRules = $state<Record<string, { id: string; yaml: string; path?: string }>>({});
	let synthesizeError = $state<Record<string, string>>({});

	const findingsList = $derived(review?.findings ?? []);

	const indexedFindings = $derived(
		findingsList
			.filter((f): f is Finding => Boolean(f))
			.map((finding, index) => ({
				finding,
				findingKey: `${finding.file}:${finding.line}:${finding.title}:${index}`
			}))
	);

	const filteredFindings = $derived(
		filter === 'all'
			? indexedFindings
			: indexedFindings.filter(({ finding }) => severityBucket(finding.severity) === filter)
	);

	const criticalCount = $derived(
		findingsList.filter((f) => severityBucket(f.severity) === 'critical').length
	);
	const majorCount = $derived(
		findingsList.filter((f) => severityBucket(f.severity) === 'major').length
	);
	const minorCount = $derived(
		findingsList.filter((f) => severityBucket(f.severity) === 'minor').length
	);

	const filterPrunedCount = $derived(
		Math.max(0, (review.pass1_candidates ?? 0) - (review.pass2_verified ?? 0))
	);

	const filterRatePercent = $derived(
		computeNoiseReductionPct(review.pass1_candidates, review.pass2_verified)
	);

	const linesAdded = $derived(review.lines_added);
	const linesRemoved = $derived(review.lines_removed);

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
Task for Agent:
Please review this file, apply the recommended fix or architectural adjustment, and verify that the tests and checks pass.`;

		navigator.clipboard.writeText(prompt).then(() => {
			copiedPromptId = id;
			setTimeout(() => {
				if (copiedPromptId === id) copiedPromptId = null;
			}, 2000);
		});
	}

	async function markFalsePositive(finding: Finding, id: string) {
		markingFalsePositive = id;
		feedbackError[id] = '';
		try {
			const res = await apiFetch('/api/feedback', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({
					review_id: review.id,
					repo: review.repo,
					file_path: finding.file,
					issue_title: finding.title,
					feedback_type: 'false_positive',
					reason: 'Dismissed by engineer via Revue web dashboard'
				})
			});
			if (res.ok) {
				localDismissed[id] = true;
				await invalidate('app:reviews');
			} else {
				const errText = await res.text();
				feedbackError[id] = errText || `Failed to save feedback (HTTP ${res.status})`;
			}
		} catch (e) {
			feedbackError[id] = e instanceof Error ? e.message : 'Network error submitting feedback';
		} finally {
			markingFalsePositive = null;
		}
	}

	async function synthesizeAstRule(finding: Finding, id: string) {
		synthesizingRule = id;
		synthesizeError[id] = '';
		try {
			const res = await apiFetch('/api/rules/synthesize', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({
					repo: review.repo,
					file: finding.file,
					line: finding.line,
					title: finding.title,
					comment: finding.body,
					snippet: finding.snippet,
					commit: true
				})
			});
			const data = await res.json();
			if (res.ok && data.success) {
				synthesizedRules[id] = {
					id: data.rule_id,
					yaml: data.yaml,
					path: data.saved_path
				};
			} else {
				synthesizeError[id] = data.error || 'Failed to synthesize valid Semgrep rule';
			}
		} catch (e) {
			synthesizeError[id] =
				e instanceof Error ? e.message : 'Network error synthesizing rule';
		} finally {
			synthesizingRule = null;
		}
	}
</script>

<dialog
	{@attach modalDialog(onClose)}
	class="review-dialog"
	closedby="any"
	aria-labelledby="review-dialog-title"
	onclose={onClose}
>
	<div class="modal-header">
		<div class="title-meta">
			<span class="forge-tag {review.forge}">{review.forge}</span>
			<h2 id="review-dialog-title">{review.repo} #{review.pr_number}</h2>
			{#if review.title}
				<span class="pr-title">{review.title}</span>
			{/if}
		</div>
		<button type="button" class="close-btn" onclick={onClose} aria-label="Close review details">
			✕
		</button>
	</div>

	<div class="hero-stats">
		<div class="hero-card">
			<div class="hero-label">Lines Changed</div>
			{#if linesAdded !== undefined && linesRemoved !== undefined}
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
			{:else}
				<div class="hero-val">
					<span>—</span>
				</div>
			{/if}
		</div>

		<div class="hero-card">
			<div class="hero-label">Findings Breakdown</div>
			<div class="severity-chips">
				{#if criticalCount > 0}
					<span class="sev-chip critical">{criticalCount} Critical</span>
				{/if}
				<span class="sev-chip major">{majorCount} Major</span>
				<span class="sev-chip minor">{minorCount} Minor</span>
			</div>
			<div class="hero-sub">{findingsList.length} actionable suggestions</div>
		</div>

		<div class="hero-card highlight">
			<div class="hero-label">Two-Pass Judge Efficiency</div>
			<div class="hero-val rate">
				{filterRatePercent !== null ? `${filterRatePercent}% Pruned` : '—'}
			</div>
			<div class="hero-sub">
				{review.pass1_candidates} candidates → {review.pass2_verified} verified ({filterPrunedCount}
				false positives pruned)
			</div>
		</div>
	</div>

	<div class="modal-body">
		<section class="summary-section">
			<div class="section-title-wrap">
				<span class="icon">✨</span>
				<h3>AI Executive Summary</h3>
			</div>
			<div class="summary-content">
				<p>{review.summary}</p>
			</div>
		</section>

		<section class="findings-section">
			<div class="findings-header">
				<h3>Findings ({filteredFindings.length})</h3>
				<div class="filter-pills">
					<button
						type="button"
						class="pill"
						class:active={filter === 'all'}
						onclick={() => (filter = 'all')}
					>
						All ({findingsList.length})
					</button>
					{#if criticalCount > 0}
						<button
							type="button"
							class="pill"
							class:active={filter === 'critical'}
							onclick={() => (filter = 'critical')}
						>
							Critical ({criticalCount})
						</button>
					{/if}
					<button
						type="button"
						class="pill"
						class:active={filter === 'major'}
						onclick={() => (filter = 'major')}
					>
						Major ({majorCount})
					</button>
					<button
						type="button"
						class="pill"
						class:active={filter === 'minor'}
						onclick={() => (filter = 'minor')}
					>
						Minor ({minorCount})
					</button>
				</div>
			</div>

			<div class="findings-list">
				{#if filteredFindings.length === 0}
					<div class="empty-state">No findings matching this filter. Clean diff! ✨</div>
				{:else}
					{#each filteredFindings as { finding, findingKey } (findingKey)}
						{@const bucket = severityBucket(finding.severity)}
						{@const isDismissed = Boolean(
							localDismissed[findingKey] ||
								finding.feedback === 'false_positive' ||
								finding.feedback === 'dismissed'
						)}
						<div class="finding-card {bucket}">
							<div class="finding-top">
								<span class="severity-badge {bucket}">
									{finding.severity.toUpperCase()}
								</span>
								<span class="category-badge">
									{finding.category ?? 'Code Review'}
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

							<div class="finding-actions">
								<button
									type="button"
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
									type="button"
									class="action-btn agent-btn"
									onclick={() => copyAgentPrompt(finding, findingKey)}
									title="Copy pre-formatted prompt for your coding agent to apply this fix"
								>
									{#if copiedPromptId === findingKey}
										<span class="success-icon">✓</span> Copied Agent Prompt!
									{:else}
										<span>🤖</span> Prompt for AI Agent
									{/if}
								</button>

								<button
									type="button"
									class="action-btn ast-btn"
									disabled={synthesizingRule === findingKey}
									onclick={() => synthesizeAstRule(finding, findingKey)}
									title="Synthesize a deterministic Semgrep AST YAML rule so this anti-pattern is caught locally"
								>
									{#if synthesizingRule === findingKey}
										<span>⚡</span> Synthesizing...
									{:else if synthesizedRules[findingKey]}
										<span class="success-icon">✓</span> Rule Learned!
									{:else}
										<span>⚡</span> Synthesize AST Rule
									{/if}
								</button>

								{#if isDismissed}
									<span
										class="feedback-badge dismissed"
										title="Revue learned this was a false alarm and will prune similar findings for {review.repo}"
									>
										<span>🚫</span> Dismissed as False Alarm (Learned)
									</span>
								{:else}
									<button
										type="button"
										class="action-btn dismiss-btn"
										disabled={markingFalsePositive === findingKey}
										onclick={() => markFalsePositive(finding, findingKey)}
										title="Mark this finding as a false positive so future reviews of {review.repo} suppress it"
									>
										{#if markingFalsePositive === findingKey}
											<span>⏳</span> Saving...
										{:else}
											<span>👎</span> False Alarm
										{/if}
									</button>
								{/if}
							</div>

							{#if feedbackError[findingKey]}
								<div class="synthesized-error-box" role="alert">
									<span>⚠️ {feedbackError[findingKey]}</span>
								</div>
							{/if}

							{#if synthesizedRules[findingKey]}
								<div class="synthesized-rule-box">
									<div class="synthesized-header">
										<span class="rule-badge">
											🎯 Learned AST Rule: <code>{synthesizedRules[findingKey].id}</code>
										</span>
										{#if synthesizedRules[findingKey].path}
											<span class="rule-path">
												Saved to <code>{synthesizedRules[findingKey].path}</code>
											</span>
										{/if}
									</div>
									<pre class="rule-yaml"><code>{synthesizedRules[findingKey].yaml}</code></pre>
								</div>
							{:else if synthesizeError[findingKey]}
								<div class="synthesized-error-box" role="alert">
									<span>⚠️ {synthesizeError[findingKey]}</span>
								</div>
							{/if}
						</div>
					{/each}
				{/if}
			</div>
		</section>
	</div>
</dialog>

<style>
	.review-dialog {
		padding: 0;
		border: 1px solid var(--color-outline);
		border-radius: 1rem;
		background: var(--color-bg);
		color: var(--color-fg);
		max-width: min(920px, calc(100vw - 2rem));
		width: 100%;
		max-height: 92vh;
		overflow: hidden;
		box-shadow: var(--shadow-elevation-3);
	}

	.review-dialog[open] {
		display: flex;
		flex-direction: column;
	}

	.modal-header {
		padding: 1.25rem 1.75rem;
		border-bottom: 1px solid var(--color-line);
		display: flex;
		align-items: center;
		justify-content: space-between;
		background: var(--color-surface);
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
		color: var(--color-heading);
		margin: 0;
	}

	.pr-title {
		color: var(--color-muted);
		font-size: 0.95rem;
	}

	.forge-tag {
		font-size: 0.7rem;
		font-weight: 600;
		padding: 0.15rem 0.5rem;
		border-radius: 9999px;
		text-transform: uppercase;
		background: var(--color-surface-high);
		color: var(--color-muted);
	}

	.forge-tag.forgejo {
		background: var(--color-status-success-soft);
		color: var(--color-status-success);
	}

	.close-btn {
		background: none;
		border: none;
		color: var(--color-muted);
		font-size: 1.25rem;
		cursor: pointer;
		padding: 0.25rem 0.5rem;
		border-radius: 0.35rem;
		transition: all 0.15s ease;
	}

	.close-btn:hover {
		color: var(--color-heading);
		background: var(--color-surface-high);
	}

	.hero-stats {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: 1rem;
		padding: 1.25rem 1.75rem;
		background: var(--color-surface);
		border-bottom: 1px solid var(--color-line);
	}

	.hero-card {
		background: var(--color-bg);
		border: 1px solid var(--color-line);
		border-radius: 0.75rem;
		padding: 1rem 1.15rem;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.hero-card.highlight {
		border-color: var(--color-accent);
		background: linear-gradient(135deg, var(--color-accent-soft) 0%, var(--color-bg) 100%);
	}

	.hero-label {
		font-size: 0.75rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--color-muted);
	}

	.hero-val {
		font-size: 1.35rem;
		font-weight: 700;
		color: var(--color-heading);
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.hero-val .add {
		color: var(--color-status-success);
	}

	.hero-val .del {
		color: var(--color-status-danger);
	}

	.hero-val.rate {
		color: var(--color-accent);
	}

	.delta-bar-wrap {
		width: 100%;
		height: 6px;
		background: var(--color-surface-high);
		border-radius: 9999px;
		display: flex;
		overflow: hidden;
		margin-top: 0.25rem;
	}

	.delta-bar-add {
		background: var(--color-status-success);
		height: 100%;
	}

	.delta-bar-del {
		background: var(--color-status-danger);
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
		background: var(--color-status-danger-soft);
		color: var(--color-status-danger);
	}

	.sev-chip.major {
		background: var(--color-status-warning-soft);
		color: var(--color-status-warning);
	}

	.sev-chip.minor {
		background: var(--color-status-info-soft);
		color: var(--color-status-info);
	}

	.hero-sub {
		font-size: 0.75rem;
		color: var(--color-muted);
	}

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
		color: var(--color-heading);
		margin: 0;
	}

	.summary-content {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.75rem;
		padding: 1.15rem;
		color: var(--color-fg);
		font-size: 0.9rem;
		line-height: 1.6;
	}

	.summary-content p {
		margin: 0;
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
		flex-wrap: wrap;
	}

	.pill {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		color: var(--color-muted);
		padding: 0.3rem 0.75rem;
		border-radius: 9999px;
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.pill:hover,
	.pill.active {
		background: var(--color-surface-high);
		color: var(--color-heading);
		border-color: var(--color-outline);
	}

	.findings-list {
		display: flex;
		flex-direction: column;
		gap: 1.15rem;
	}

	.finding-card {
		border: 1px solid var(--color-line);
		background: var(--color-surface);
		border-radius: 0.75rem;
		padding: 1.25rem;
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.finding-card.critical {
		border-left: 4px solid var(--color-status-danger);
	}

	.finding-card.major {
		border-left: 4px solid var(--color-status-warning);
	}

	.finding-card.minor {
		border-left: 4px solid var(--color-status-info);
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

	.severity-badge.critical {
		background: var(--color-status-danger-soft);
		color: var(--color-status-danger);
	}

	.severity-badge.major {
		background: var(--color-status-warning-soft);
		color: var(--color-status-warning);
	}

	.severity-badge.minor {
		background: var(--color-status-info-soft);
		color: var(--color-status-info);
	}

	.category-badge {
		font-size: 0.7rem;
		font-weight: 600;
		background: var(--color-surface-high);
		color: var(--color-muted);
		padding: 0.2rem 0.55rem;
		border-radius: 9999px;
	}

	.location code {
		font-size: 0.8rem;
		font-family: var(--font-mono);
		color: var(--color-status-info);
		background: var(--color-status-info-soft);
		padding: 0.2rem 0.5rem;
		border-radius: 0.35rem;
	}

	.confidence {
		font-size: 0.75rem;
		color: var(--color-muted);
		margin-left: auto;
	}

	h4 {
		font-size: 1.05rem;
		font-weight: 600;
		color: var(--color-heading);
		margin: 0;
	}

	.finding-body {
		font-size: 0.875rem;
		color: var(--color-fg);
		margin: 0;
		line-height: 1.6;
	}

	.snippet-wrap {
		border: 1px solid var(--color-line);
		border-radius: 0.5rem;
		overflow: hidden;
		background: var(--color-bg);
	}

	.snippet-header {
		padding: 0.45rem 0.85rem;
		background: var(--color-surface-high);
		border-bottom: 1px solid var(--color-line);
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--color-muted);
	}

	.snippet {
		padding: 0.85rem;
		font-family: var(--font-mono);
		font-size: 0.825rem;
		color: var(--color-heading);
		overflow-x: auto;
		margin: 0;
	}

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
		border: 1px solid var(--color-outline);
	}

	.patch-btn {
		background: var(--color-surface-high);
		color: var(--color-heading);
	}

	.patch-btn:hover {
		border-color: var(--color-accent);
	}

	.agent-btn {
		background: var(--color-accent-soft);
		border-color: var(--color-accent);
		color: var(--color-accent-soft-fg);
	}

	.agent-btn:hover {
		background: var(--color-accent);
		color: var(--color-accent-fg);
	}

	.ast-btn {
		background: var(--color-status-info-soft);
		border-color: var(--color-status-info-border);
		color: var(--color-status-info);
	}

	.ast-btn:hover:not(:disabled) {
		border-color: var(--color-status-info);
	}

	.ast-btn:disabled {
		opacity: 0.6;
		cursor: wait;
	}

	.synthesized-rule-box {
		margin-top: 0.5rem;
		background: var(--color-bg);
		border: 1px solid var(--color-status-info-border);
		border-radius: 0.5rem;
		padding: 0.85rem;
	}

	.synthesized-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-bottom: 0.6rem;
		font-size: 0.8rem;
	}

	.rule-badge {
		color: var(--color-status-info);
		font-weight: 600;
	}

	.rule-badge code {
		font-family: var(--font-mono);
		background: var(--color-status-info-soft);
		padding: 0.15rem 0.4rem;
		border-radius: 0.3rem;
	}

	.rule-path {
		color: var(--color-muted);
		font-size: 0.75rem;
	}

	.rule-yaml {
		margin: 0;
		padding: 0.75rem;
		background: var(--color-surface-high);
		border-radius: 0.4rem;
		overflow-x: auto;
		font-family: var(--font-mono);
		font-size: 0.78rem;
		line-height: 1.4;
		color: var(--color-heading);
	}

	.synthesized-error-box {
		margin-top: 0.5rem;
		padding: 0.5rem 0.8rem;
		background: var(--color-status-danger-soft);
		border: 1px solid var(--color-status-danger-border);
		border-radius: 0.4rem;
		color: var(--color-status-danger);
		font-size: 0.8rem;
	}

	.dismiss-btn {
		background: var(--color-status-danger-soft);
		border-color: var(--color-status-danger-border);
		color: var(--color-status-danger);
	}

	.dismiss-btn:hover:not(:disabled) {
		border-color: var(--color-status-danger);
	}

	.dismiss-btn:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.feedback-badge.dismissed {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		padding: 0.45rem 0.85rem;
		border-radius: 0.45rem;
		background: var(--color-status-ai-soft);
		border: 1px solid var(--color-status-ai-border);
		color: var(--color-status-ai);
		font-size: 0.8rem;
		font-weight: 500;
	}

	.success-icon {
		color: var(--color-status-success);
		font-weight: 700;
	}

	.empty-state {
		padding: 2.5rem;
		text-align: center;
		color: var(--color-muted);
		font-size: 0.9rem;
	}
</style>
