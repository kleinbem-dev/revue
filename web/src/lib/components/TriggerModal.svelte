<script lang="ts">
	let {
		isOpen,
		onClose,
		onTrigger
	}: {
		isOpen: boolean;
		onClose: () => void;
		onTrigger: (repo: string, pr: number) => Promise<void>;
	} = $props();

	let repo = $state('');
	let pr = $state<number | ''>('');
	let isSubmitting = $state(false);
	let error = $state<string | null>(null);

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (!repo || !pr) {
			error = 'Please provide both repository and PR number.';
			return;
		}

		isSubmitting = true;
		error = null;
		try {
			await onTrigger(repo.trim(), Number(pr));
			onClose();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to trigger review';
		} finally {
			isSubmitting = false;
		}
	}
</script>

<svelte:window
	onkeydown={(e) => {
		if (isOpen && e.key === 'Escape') onClose();
	}}
/>

{#if isOpen}
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
				<h3>Trigger On-Demand Review</h3>
				<button class="close-btn" onclick={onClose} aria-label="Close">✕</button>
			</div>

			<form onsubmit={handleSubmit} class="modal-body">
				<p class="desc">
					Manually trigger the 2-pass review pipeline (Pass 1 Detective + Pass 2 Judge) on any
					Pull Request on GitHub or Forgejo.
				</p>

				{#if error}
					<div class="error-banner">{error}</div>
				{/if}

				<div class="field">
					<label for="repo-input">Repository</label>
					<input
						id="repo-input"
						type="text"
						bind:value={repo}
						placeholder="e.g. kleinbem/nix-config or owner/repo"
						required
					/>
				</div>

				<div class="field">
					<label for="pr-input">Pull Request Number</label>
					<input
						id="pr-input"
						type="number"
						bind:value={pr}
						placeholder="e.g. 42"
						min="1"
						required
					/>
				</div>

				<div class="modal-actions">
					<button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
					<button type="submit" class="btn btn-primary" disabled={isSubmitting}>
						{isSubmitting ? 'Queueing Review...' : 'Start Review 🚀'}
					</button>
				</div>
			</form>
		</div>
	</div>
{/if}

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
		padding: 1rem;
	}

	.modal {
		background: #0f172a;
		border: 1px solid #1e293b;
		border-radius: 0.85rem;
		max-width: 480px;
		width: 100%;
		overflow: hidden;
		box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.5);
	}

	.modal-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1.15rem 1.5rem;
		border-bottom: 1px solid #1e293b;
	}

	h3 {
		font-size: 1.05rem;
		font-weight: 600;
		color: #f8fafc;
		margin: 0;
	}

	.close-btn {
		background: none;
		border: none;
		color: #64748b;
		font-size: 1.1rem;
		cursor: pointer;
		padding: 0.25rem;
		border-radius: 0.25rem;
	}

	.close-btn:hover {
		color: #f1f5f9;
		background: #1e293b;
	}

	.modal-body {
		padding: 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 1.15rem;
	}

	.desc {
		font-size: 0.85rem;
		color: #94a3b8;
		margin: 0;
		line-height: 1.5;
	}

	.error-banner {
		background: rgba(239, 68, 68, 0.15);
		border: 1px solid rgba(239, 68, 68, 0.3);
		color: #f87171;
		font-size: 0.825rem;
		padding: 0.65rem 0.85rem;
		border-radius: 0.45rem;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
	}

	label {
		font-size: 0.8rem;
		font-weight: 500;
		color: #cbd5e1;
	}

	input {
		background: #020617;
		border: 1px solid #334155;
		border-radius: 0.45rem;
		padding: 0.55rem 0.85rem;
		font-size: 0.875rem;
		color: #f8fafc;
		outline: none;
		transition: border-color 0.15s ease;
	}

	input:focus {
		border-color: #8b5cf6;
		box-shadow: 0 0 0 1px #8b5cf6;
	}

	.modal-actions {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 0.75rem;
		margin-top: 0.5rem;
	}

	.btn {
		padding: 0.55rem 1.15rem;
		border-radius: 0.45rem;
		font-size: 0.85rem;
		font-weight: 500;
		cursor: pointer;
		transition: all 0.15s ease;
		border: 1px solid transparent;
	}

	.btn-secondary {
		background: #1e293b;
		border-color: #334155;
		color: #cbd5e1;
	}

	.btn-secondary:hover {
		background: #334155;
		color: #f8fafc;
	}

	.btn-primary {
		background: #8b5cf6;
		color: white;
	}

	.btn-primary:hover:not(:disabled) {
		background: #7c3aed;
	}

	.btn-primary:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}
</style>
