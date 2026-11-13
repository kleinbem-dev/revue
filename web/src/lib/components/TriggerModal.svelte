<script lang="ts">
	import {
		modalDialog,
		type ProviderStatus,
		type ReviewRecord
	} from '#lib';

	let {
		isOpen = true,
		providers = [],
		recentReviews = [],
		onClose,
		onTrigger
	}: {
		isOpen?: boolean;
		providers?: ProviderStatus[];
		recentReviews?: ReviewRecord[];
		onClose: () => void;
		onTrigger: (repo: string, pr: number, publish: boolean) => Promise<void>;
	} = $props();

	let repo = $state('');
	let pr = $state<number | ''>('');
	let publish = $state(false);
	let isSubmitting = $state(false);
	let error = $state<string | null>(null);

	const recentTargets = $derived.by(() => {
		const out: Array<{ repo: string; pr: number }> = [];
		for (const item of recentReviews) {
			if (!item.repo || !item.pr_number) continue;
			if (out.some((t) => t.repo === item.repo && t.pr === item.pr_number)) continue;
			out.push({ repo: item.repo, pr: item.pr_number });
			if (out.length >= 4) break;
		}
		return out;
	});

	const cascadeSummary = $derived(
		providers.length > 0
			? providers.map((p) => p.name).join(' → ')
			: 'Configured provider cascade'
	);

	function selectTarget(targetRepo: string, targetPr: number) {
		repo = targetRepo;
		pr = targetPr;
		error = null;
	}

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (!repo.trim() || !pr || Number(pr) <= 0) {
			error = 'Please provide a valid owner/repo slug and positive PR number.';
			return;
		}

		isSubmitting = true;
		error = null;
		try {
			await onTrigger(repo.trim(), Number(pr), publish);
			onClose();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to trigger review';
		} finally {
			isSubmitting = false;
		}
	}
</script>

{#if isOpen}
	<dialog
		{@attach modalDialog(onClose)}
		class="trigger-dialog"
		closedby="any"
		aria-labelledby="trigger-dialog-title"
		onclose={onClose}
	>
		<div class="modal-header">
			<div class="header-left">
				<span class="header-icon">🚀</span>
				<h3 id="trigger-dialog-title">Trigger On-Demand Review</h3>
			</div>
			<button type="button" class="close-btn" onclick={onClose} aria-label="Close">✕</button>
		</div>

		<form onsubmit={handleSubmit} class="modal-body">
			<p class="desc">
				Manually trigger the two-pass review pipeline (<strong>Pass 1 Detective</strong> +
				<strong>Pass 2 Judge</strong>) on any pull request. Episodic constraints and matching skills
				are injected automatically.
			</p>

			{#if error}
				<div class="error-banner" role="alert">{error}</div>
			{/if}

			{#if recentTargets.length > 0}
				<div class="preset-section">
					<span class="preset-label">Recent Pull Requests:</span>
					<div class="preset-chips">
						{#each recentTargets as target (`${target.repo}#${target.pr}`)}
							<button
								type="button"
								class="chip-btn"
								class:selected={repo === target.repo && pr === target.pr}
								onclick={() => selectTarget(target.repo, target.pr)}
							>
								<span class="chip-pr">#{target.pr}</span>
								<span class="chip-repo">{target.repo}</span>
							</button>
						{/each}
					</div>
				</div>
			{/if}

			<div class="field">
				<label for="repo-input">Repository</label>
				<input
					id="repo-input"
					type="text"
					bind:value={repo}
					placeholder="owner/repo"
					required
				/>
			</div>

			<div class="field">
				<label for="pr-input">Pull Request Number</label>
				<input
					id="pr-input"
					type="number"
					bind:value={pr}
					placeholder="42"
					min="1"
					required
				/>
			</div>

			<div class="publish-box">
				<label class="publish-label">
					<input type="checkbox" bind:checked={publish} class="publish-checkbox" />
					<div class="publish-text">
						<span class="publish-title">Post Review Directly to Forge</span>
						<span class="publish-subtitle">
							Submit verified findings & line diff suggestions to GitHub / Forgejo PR
						</span>
					</div>
				</label>
			</div>

			<div class="pipeline-preview">
				<span class="pipeline-label">Provider Cascade:</span>
				<div class="pipeline-badge">
					<span class="dot live"></span>
					<span>{cascadeSummary}</span>
				</div>
			</div>

			<div class="modal-actions">
				<button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
				<button type="submit" class="btn btn-primary" disabled={isSubmitting}>
					{#if isSubmitting}
						<span class="spinner"></span> Starting Review...
					{:else}
						<span>Execute Review 🚀</span>
					{/if}
				</button>
			</div>
		</form>
	</dialog>
{/if}

<style>
	.trigger-dialog {
		padding: 0;
		background: var(--color-bg);
		color: var(--color-fg);
		border: 1px solid var(--color-outline);
		border-radius: 0.85rem;
		max-width: min(520px, calc(100vw - 2rem));
		width: 100%;
		overflow: hidden;
		box-shadow: var(--shadow-elevation-3);
	}

	.trigger-dialog[open] {
		display: flex;
		flex-direction: column;
	}

	.modal-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1.15rem 1.5rem;
		border-bottom: 1px solid var(--color-line);
		background: var(--color-surface);
	}

	.header-left {
		display: flex;
		align-items: center;
		gap: 0.65rem;
	}

	.header-icon {
		font-size: 1.25rem;
	}

	h3 {
		font-size: 1.05rem;
		font-weight: 600;
		color: var(--color-heading);
		margin: 0;
	}

	.close-btn {
		background: none;
		border: none;
		color: var(--color-muted);
		font-size: 1.1rem;
		cursor: pointer;
		padding: 0.25rem 0.5rem;
		border-radius: 0.25rem;
		transition: all 0.15s ease;
	}

	.close-btn:hover {
		color: var(--color-heading);
		background: var(--color-surface-high);
	}

	.modal-body {
		padding: 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 1.15rem;
		overflow-y: auto;
	}

	.desc {
		font-size: 0.85rem;
		color: var(--color-muted);
		margin: 0;
		line-height: 1.5;
	}

	.desc strong {
		color: var(--color-heading);
	}

	.error-banner {
		background: var(--color-status-danger-soft);
		border: 1px solid var(--color-status-danger-border);
		color: var(--color-status-danger);
		font-size: 0.825rem;
		padding: 0.65rem 0.85rem;
		border-radius: 0.45rem;
	}

	.preset-section {
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
	}

	.preset-label {
		font-size: 0.75rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--color-muted);
	}

	.preset-chips {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.chip-btn {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.45rem;
		padding: 0.35rem 0.65rem;
		color: var(--color-fg);
		font-size: 0.775rem;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 0.4rem;
		transition: all 0.15s ease;
	}

	.chip-btn:hover {
		background: var(--color-surface-high);
		border-color: var(--color-outline);
		color: var(--color-heading);
	}

	.chip-btn.selected {
		background: var(--color-accent-soft);
		border-color: var(--color-accent);
		color: var(--color-accent-soft-fg);
	}

	.chip-pr {
		font-weight: 700;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
	}

	label {
		font-size: 0.8rem;
		font-weight: 500;
		color: var(--color-fg);
	}

	input[type='text'],
	input[type='number'] {
		background: var(--color-surface);
		border: 1px solid var(--color-outline);
		border-radius: 0.45rem;
		padding: 0.6rem 0.85rem;
		font-size: 0.875rem;
		color: var(--color-heading);
		outline: none;
		transition: border-color 0.15s ease;
	}

	input[type='text']:focus,
	input[type='number']:focus {
		border-color: var(--color-accent);
	}

	.publish-box {
		background: var(--color-status-info-soft);
		border: 1px solid var(--color-status-info-border);
		border-radius: 0.5rem;
		padding: 0.75rem 0.9rem;
	}

	.publish-label {
		display: flex;
		align-items: flex-start;
		gap: 0.75rem;
		cursor: pointer;
		user-select: none;
	}

	.publish-checkbox {
		margin-top: 0.2rem;
		width: 1.05rem;
		height: 1.05rem;
		accent-color: var(--color-accent);
		cursor: pointer;
	}

	.publish-text {
		display: flex;
		flex-direction: column;
		gap: 0.15rem;
	}

	.publish-title {
		font-size: 0.85rem;
		font-weight: 600;
		color: var(--color-heading);
	}

	.publish-subtitle {
		font-size: 0.72rem;
		color: var(--color-muted);
		line-height: 1.3;
	}

	.pipeline-preview {
		display: flex;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 0.5rem;
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.45rem;
		padding: 0.65rem 0.85rem;
		font-size: 0.775rem;
	}

	.pipeline-label {
		color: var(--color-muted);
	}

	.pipeline-badge {
		display: flex;
		align-items: center;
		gap: 0.45rem;
		color: var(--color-heading);
		font-weight: 500;
	}

	.dot.live {
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--color-status-success);
	}

	.modal-actions {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 0.75rem;
		margin-top: 0.25rem;
	}

	.btn {
		padding: 0.6rem 1.25rem;
		border-radius: 0.45rem;
		font-size: 0.85rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
		border: 1px solid transparent;
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.btn-secondary {
		background: var(--color-surface);
		border-color: var(--color-line);
		color: var(--color-fg);
	}

	.btn-secondary:hover {
		background: var(--color-surface-high);
		color: var(--color-heading);
	}

	.btn-primary {
		background: var(--color-accent);
		color: var(--color-accent-fg);
	}

	.btn-primary:hover:not(:disabled) {
		background: var(--color-accent-hover);
	}

	.btn-primary:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.spinner {
		width: 14px;
		height: 14px;
		border: 2px solid rgba(255, 255, 255, 0.35);
		border-top-color: currentColor;
		border-radius: 50%;
		animation: spin 0.8s linear infinite;
	}

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
</style>
