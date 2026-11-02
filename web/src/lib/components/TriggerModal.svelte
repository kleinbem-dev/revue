<script lang="ts">
	let {
		isOpen = true,
		onClose,
		onTrigger
	}: {
		isOpen?: boolean;
		onClose: () => void;
		onTrigger: (repo: string, pr: number, publish: boolean) => Promise<void>;
	} = $props();

	let repo = $state('kleinbem/nix-config');
	let pr = $state<number | ''>(516);
	let publish = $state(false);
	let isSubmitting = $state(false);
	let error = $state<string | null>(null);

	const presets = [
		{ repo: 'kleinbem/nix-config', pr: 516, desc: 'feat: containerize revue service' },
		{ repo: 'kleinbem/nix-config', pr: 505, desc: 'feat: enable forgejo & revue daemon' },
		{ repo: 'kleinbem/revue', pr: 1, desc: 'feat: add Brain & Skills directory' }
	];

	function selectPreset(presetRepo: string, presetPr: number) {
		repo = presetRepo;
		pr = presetPr;
		error = null;
	}

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (!repo || !pr) {
			error = 'Please provide both repository and PR number.';
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
				<div class="header-left">
					<span class="header-icon">🚀</span>
					<h3>Trigger On-Demand Review</h3>
				</div>
				<button class="close-btn" onclick={onClose} aria-label="Close">✕</button>
			</div>

			<form onsubmit={handleSubmit} class="modal-body">
				<p class="desc">
					Manually trigger the 2-pass review pipeline (<strong>Pass 1 Detective</strong> + <strong>Pass 2 Judge</strong>) on any pull request. Episodic brain constraints and fleet skills will be injected automatically.
				</p>

				{#if error}
					<div class="error-banner">{error}</div>
				{/if}

				<!-- Preset Quick Chips -->
				<div class="preset-section">
					<span class="preset-label">Quick Presets:</span>
					<div class="preset-chips">
						{#each presets as preset}
							<button
								type="button"
								class="chip-btn"
								class:selected={repo === preset.repo && pr === preset.pr}
								onclick={() => selectPreset(preset.repo, preset.pr)}
							>
								<span class="chip-pr">#{preset.pr}</span>
								<span class="chip-repo">{preset.repo.replace('kleinbem/', '')}</span>
							</button>
						{/each}
					</div>
				</div>

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
						placeholder="e.g. 516"
						min="1"
						required
					/>
				</div>

				<div class="publish-box">
					<label class="publish-label">
						<input type="checkbox" bind:checked={publish} class="publish-checkbox" />
						<div class="publish-text">
							<span class="publish-title">Post Review Directly to Forge</span>
							<span class="publish-subtitle">Submit verified findings & line diff suggestions to GitHub / Forgejo PR</span>
						</div>
					</label>
				</div>

				<div class="pipeline-preview">
					<span class="pipeline-label">Execution Strategy:</span>
					<div class="pipeline-badge">
						<span class="dot live"></span>
						<span>Gemini 2.5 Pro → Mesh-LLM P2P GPU (Fallback)</span>
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
		</div>
	</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.75);
		backdrop-filter: blur(8px);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 1000;
		padding: 1rem;
	}

	.modal {
		background: #11151c;
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 0.85rem;
		max-width: 500px;
		width: 100%;
		overflow: hidden;
		box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.7);
	}

	.modal-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1.15rem 1.5rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
		background: #161c26;
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
		color: #f0f6fc;
		margin: 0;
	}

	.close-btn {
		background: none;
		border: none;
		color: #8b949e;
		font-size: 1.1rem;
		cursor: pointer;
		padding: 0.25rem 0.5rem;
		border-radius: 0.25rem;
		transition: all 0.15s ease;
	}

	.close-btn:hover {
		color: #f0f6fc;
		background: rgba(255, 255, 255, 0.08);
	}

	.modal-body {
		padding: 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 1.15rem;
	}

	.desc {
		font-size: 0.85rem;
		color: #8b949e;
		margin: 0;
		line-height: 1.5;
	}

	.desc strong {
		color: #f0f6fc;
	}

	.error-banner {
		background: rgba(248, 81, 73, 0.15);
		border: 1px solid rgba(248, 81, 73, 0.3);
		color: #ff7b72;
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
		color: #8b949e;
	}

	.preset-chips {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.chip-btn {
		background: #1c2331;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.45rem;
		padding: 0.35rem 0.65rem;
		color: #c9d1d9;
		font-size: 0.775rem;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 0.4rem;
		transition: all 0.15s ease;
	}

	.chip-btn:hover {
		background: #252f42;
		border-color: rgba(255, 255, 255, 0.2);
		color: #f0f6fc;
	}

	.chip-btn.selected {
		background: rgba(88, 166, 255, 0.15);
		border-color: #58a6ff;
		color: #58a6ff;
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
		color: #c9d1d9;
	}

	input {
		background: #090d13;
		border: 1px solid rgba(255, 255, 255, 0.15);
		border-radius: 0.45rem;
		padding: 0.6rem 0.85rem;
		font-size: 0.875rem;
		color: #f0f6fc;
		outline: none;
		transition: border-color 0.15s ease;
	}

	input:focus {
		border-color: #58a6ff;
		box-shadow: 0 0 0 2px rgba(88, 166, 255, 0.2);
	}

	.publish-box {
		background: rgba(31, 111, 235, 0.08);
		border: 1px solid rgba(56, 139, 253, 0.3);
		border-radius: 0.5rem;
		padding: 0.75rem 0.9rem;
		transition: all 0.2s ease;
	}

	.publish-box:hover {
		background: rgba(31, 111, 235, 0.12);
		border-color: rgba(56, 139, 253, 0.5);
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
		accent-color: #388bfd;
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
		color: #e6edf3;
	}

	.publish-subtitle {
		font-size: 0.72rem;
		color: #8b949e;
		line-height: 1.3;
	}

	.pipeline-preview {
		display: flex;
		align-items: center;
		justify-content: space-between;
		background: #161c26;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.45rem;
		padding: 0.65rem 0.85rem;
		font-size: 0.775rem;
	}

	.pipeline-label {
		color: #8b949e;
	}

	.pipeline-badge {
		display: flex;
		align-items: center;
		gap: 0.45rem;
		color: #f0f6fc;
		font-weight: 500;
	}

	.dot.live {
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: #3fb950;
		box-shadow: 0 0 6px #3fb950;
	}

	.modal-actions {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 0.75rem;
		margin-top: 0.5rem;
	}

	.btn {
		padding: 0.6rem 1.25rem;
		border-radius: 0.45rem;
		font-size: 0.85rem;
		font-weight: 500;
		cursor: pointer;
		transition: all 0.15s ease;
		border: 1px solid transparent;
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.btn-secondary {
		background: #1c2331;
		border-color: rgba(255, 255, 255, 0.12);
		color: #c9d1d9;
	}

	.btn-secondary:hover {
		background: #252f42;
		color: #f0f6fc;
	}

	.btn-primary {
		background: #238636;
		color: white;
	}

	.btn-primary:hover:not(:disabled) {
		background: #2ea043;
	}

	.btn-primary:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.spinner {
		width: 14px;
		height: 14px;
		border: 2px solid rgba(255, 255, 255, 0.3);
		border-top-color: white;
		border-radius: 50%;
		animation: spin 0.8s linear infinite;
	}

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
</style>
