<script lang="ts">
	import { generateRevueYaml, type WizardConfig } from '#lib';

	let config = $state<WizardConfig>({
		profile: 'chill',
		high_level_summary: true,
		anti_slop: true,
		semgrep: true,
		ignore_lockfiles: true,
		ignore_vendor: true,
		ignore_generated: true,
		custom_guidelines:
			'Enforce zero-trust network boundaries.\nNever hardcode credentials or secrets in plain text.\nRequire signed commits for production branches.'
	});

	let copied = $state(false);

	const toneByProfile: Record<WizardConfig['profile'], string> = {
		chill: 'Short and direct. Lead with the problem and the fix. No praise, no filler.',
		quiet: 'Concise and minimal. Only report actionable errors and critical issues. No praise.',
		assertive: 'Strict and thorough. Flag potential regressions, security edge cases, and subtle bugs.'
	};

	const severityByProfile: Record<WizardConfig['profile'], 'info' | 'warning' | 'error'> = {
		chill: 'warning',
		quiet: 'error',
		assertive: 'info'
	};

	const generatedYaml = $derived(
		generateRevueYaml({
			min_severity: severityByProfile[config.profile],
			summary: config.high_level_summary,
			two_pass: config.anti_slop,
			semgrep: config.semgrep,
			ignore_lockfiles: config.ignore_lockfiles,
			ignore_vendor: config.ignore_vendor,
			ignore_generated: config.ignore_generated,
			tone: toneByProfile[config.profile],
			custom_guidelines: config.custom_guidelines
		})
	);

	function copyYaml() {
		navigator.clipboard.writeText(generatedYaml).then(() => {
			copied = true;
			setTimeout(() => {
				copied = false;
			}, 2000);
		});
	}

	function downloadYaml() {
		const blob = new Blob([generatedYaml], { type: 'text/yaml' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = '.revue.yaml';
		a.click();
		URL.revokeObjectURL(url);
	}
</script>

<div class="wizard-container">
	<div class="wizard-header">
		<div class="header-left">
			<span class="sparkle" aria-hidden="true">⚙️</span>
			<div>
				<h2>Rules & Configuration Wizard</h2>
				<p class="subtitle">
					Configure AI review strictness, path filters, and repository guidelines.
				</p>
			</div>
		</div>
		<div class="header-actions">
			<button type="button" class="btn primary" onclick={copyYaml}>
				{#if copied}
					<span>✓</span> Copied .revue.yaml!
				{:else}
					<span>📋</span> Copy .revue.yaml
				{/if}
			</button>
			<button type="button" class="btn outline" onclick={downloadYaml}>
				<span>⬇</span> Download
			</button>
		</div>
	</div>

	<div class="wizard-grid">
		<div class="controls-panel">
			<div class="card">
				<div class="card-title">
					<span class="badge-num">1</span>
					<div>
						<h3>Review Profile & Strictness</h3>
						<span class="hint">Adjust the tone and sensitivity of Detective + Judge passes</span>
					</div>
				</div>

				<div class="profile-options">
					<label class="profile-card" class:selected={config.profile === 'quiet'}>
						<input type="radio" name="profile" value="quiet" bind:group={config.profile} />
						<div class="profile-info">
							<div class="p-title">Quiet</div>
							<div class="p-desc">Only critical security vulnerabilities and breaking defects. Minimal noise.</div>
						</div>
					</label>

					<label class="profile-card" class:selected={config.profile === 'chill'}>
						<input type="radio" name="profile" value="chill" bind:group={config.profile} />
						<div class="profile-info">
							<div class="p-title">Chill (Recommended)</div>
							<div class="p-desc">Balanced review. Catches logic errors, architecture regressions, and key edge cases.</div>
						</div>
					</label>

					<label class="profile-card" class:selected={config.profile === 'assertive'}>
						<input type="radio" name="profile" value="assertive" bind:group={config.profile} />
						<div class="profile-info">
							<div class="p-title">Assertive</div>
							<div class="p-desc">Deep and thorough. Flags performance edge cases, regressions, and subtle risks.</div>
						</div>
					</label>
				</div>
			</div>

			<div class="card">
				<div class="card-title">
					<span class="badge-num">2</span>
					<div>
						<h3>Verification Passes</h3>
						<span class="hint">Control executive summary, Pass 0 AST rules, and Pass 2 judge verification</span>
					</div>
				</div>

				<div class="toggles-list">
					<label class="toggle-row">
						<div>
							<div class="t-name">High-Level Executive Summary</div>
							<div class="t-sub">Include a concise impact summary on reviewed pull requests</div>
						</div>
						<input type="checkbox" bind:checked={config.high_level_summary} />
					</label>

					<label class="toggle-row">
						<div>
							<div class="t-name">Two-Pass Verification (Detective + Judge)</div>
							<div class="t-sub">Filter false positives using a second adversarial judge pass</div>
						</div>
						<input type="checkbox" bind:checked={config.anti_slop} />
					</label>

					<label class="toggle-row">
						<div>
							<div class="t-name">Deterministic Semgrep AST Scan (Pass 0)</div>
							<div class="t-sub">Run repository <code>.semgrep</code> rules before LLM evaluation</div>
						</div>
						<input type="checkbox" bind:checked={config.semgrep} />
					</label>
				</div>
			</div>

			<div class="card">
				<div class="card-title">
					<span class="badge-num">3</span>
					<div>
						<h3>File & Path Filters</h3>
						<span class="hint">Exclude generated or lockfile noise from the review diff budget</span>
					</div>
				</div>

				<div class="toggles-list">
					<label class="toggle-row">
						<div>
							<div class="t-name">Ignore Lockfiles</div>
							<div class="t-sub"><code>**/flake.lock</code>, <code>**/Cargo.lock</code>, <code>**/package-lock.json</code></div>
						</div>
						<input type="checkbox" bind:checked={config.ignore_lockfiles} />
					</label>

					<label class="toggle-row">
						<div>
							<div class="t-name">Ignore Vendored Dependencies</div>
							<div class="t-sub"><code>**/vendor/**</code>, <code>**/node_modules/**</code></div>
						</div>
						<input type="checkbox" bind:checked={config.ignore_vendor} />
					</label>

					<label class="toggle-row">
						<div>
							<div class="t-name">Ignore Build & Generated Artifacts</div>
							<div class="t-sub"><code>target/**</code>, <code>**/dist/**</code>, <code>**/*.min.js</code></div>
						</div>
						<input type="checkbox" bind:checked={config.ignore_generated} />
					</label>
				</div>
			</div>

			<div class="card">
				<div class="card-title">
					<span class="badge-num">4</span>
					<div>
						<h3>Custom Architectural Guidelines</h3>
						<span class="hint">Instructions injected into Detective and Judge evaluation prompts</span>
					</div>
				</div>

				<textarea
					class="guidelines-input"
					rows="4"
					aria-label="Custom Architectural Guidelines"
					placeholder="Enter rules, one per line..."
					bind:value={config.custom_guidelines}
				></textarea>
			</div>
		</div>

		<div class="preview-panel">
			<div class="preview-card">
				<div class="preview-header">
					<div class="file-name">
						<span aria-hidden="true">📄</span>
						<code>.revue.yaml</code>
					</div>
					<button type="button" class="copy-small-btn" onclick={copyYaml}>
						{#if copied}Copied!{:else}Copy YAML{/if}
					</button>
				</div>
				<pre class="yaml-code"><code>{generatedYaml}</code></pre>
				<div class="preview-footer">
					<span>Place this file in your repository root or run <code>revue init</code>.</span>
				</div>
			</div>
		</div>
	</div>
</div>

<style>
	.wizard-container {
		display: flex;
		flex-direction: column;
		gap: 1.75rem;
		max-width: 1200px;
		margin: 0 auto;
		width: 100%;
	}

	.wizard-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1.5rem 1.75rem;
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.85rem;
		flex-wrap: wrap;
		gap: 1rem;
	}

	.header-left {
		display: flex;
		align-items: center;
		gap: 1rem;
	}

	.sparkle {
		font-size: 2rem;
	}

	h2 {
		font-size: 1.35rem;
		font-weight: 700;
		color: var(--color-heading);
		margin: 0;
	}

	.subtitle {
		font-size: 0.875rem;
		color: var(--color-muted);
		margin: 0.25rem 0 0 0;
	}

	.header-actions {
		display: flex;
		gap: 0.75rem;
	}

	.btn {
		display: inline-flex;
		align-items: center;
		gap: 0.45rem;
		padding: 0.55rem 1.15rem;
		border-radius: 0.5rem;
		font-size: 0.85rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
		border: 1px solid transparent;
	}

	.btn.primary {
		background: var(--color-accent);
		color: var(--color-accent-fg);
	}

	.btn.primary:hover {
		background: var(--color-accent-hover);
	}

	.btn.outline {
		background: var(--color-surface-high);
		border-color: var(--color-line);
		color: var(--color-heading);
	}

	.btn.outline:hover {
		border-color: var(--color-outline);
	}

	.wizard-grid {
		display: grid;
		grid-template-columns: 1fr 420px;
		gap: 1.5rem;
		align-items: start;
	}

	@media (max-width: 960px) {
		.wizard-grid {
			grid-template-columns: 1fr;
		}
	}

	.controls-panel {
		display: flex;
		flex-direction: column;
		gap: 1.25rem;
	}

	.card {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.85rem;
		padding: 1.5rem;
		display: flex;
		flex-direction: column;
		gap: 1.15rem;
	}

	.card-title {
		display: flex;
		align-items: flex-start;
		gap: 0.75rem;
	}

	.badge-num {
		background: var(--color-accent-soft);
		color: var(--color-accent-soft-fg);
		font-size: 0.8rem;
		font-weight: 700;
		width: 24px;
		height: 24px;
		border-radius: 9999px;
		display: flex;
		align-items: center;
		justify-content: center;
		flex-shrink: 0;
		margin-top: 2px;
	}

	h3 {
		font-size: 1.05rem;
		font-weight: 600;
		color: var(--color-heading);
		margin: 0;
	}

	.hint {
		font-size: 0.8rem;
		color: var(--color-muted);
	}

	.profile-options {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.profile-card {
		display: flex;
		align-items: flex-start;
		gap: 0.85rem;
		padding: 0.85rem 1rem;
		border: 1px solid var(--color-line);
		border-radius: 0.65rem;
		background: var(--color-bg);
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.profile-card:hover {
		border-color: var(--color-outline);
	}

	.profile-card.selected {
		border-color: var(--color-accent);
		background: var(--color-accent-soft);
	}

	.profile-card input {
		margin-top: 3px;
		accent-color: var(--color-accent);
	}

	.p-title {
		font-size: 0.9rem;
		font-weight: 600;
		color: var(--color-heading);
	}

	.p-desc {
		font-size: 0.8rem;
		color: var(--color-muted);
		margin-top: 0.15rem;
	}

	.toggles-list {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.toggle-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
		padding: 0.75rem 1rem;
		background: var(--color-bg);
		border: 1px solid var(--color-line);
		border-radius: 0.65rem;
		cursor: pointer;
	}

	.toggle-row input {
		width: 18px;
		height: 18px;
		accent-color: var(--color-accent);
		cursor: pointer;
		flex-shrink: 0;
	}

	.t-name {
		font-size: 0.875rem;
		font-weight: 600;
		color: var(--color-heading);
	}

	.t-sub {
		font-size: 0.775rem;
		color: var(--color-muted);
		margin-top: 0.15rem;
	}

	.t-sub code {
		background: var(--color-surface);
		padding: 0.1rem 0.35rem;
		border-radius: 0.25rem;
		color: var(--color-accent-soft-fg);
	}

	.guidelines-input {
		width: 100%;
		background: var(--color-bg);
		border: 1px solid var(--color-line);
		border-radius: 0.5rem;
		padding: 0.85rem;
		color: var(--color-heading);
		font-family: var(--font-mono);
		font-size: 0.85rem;
		line-height: 1.5;
		resize: vertical;
		box-sizing: border-box;
	}

	.preview-panel {
		position: sticky;
		top: 5rem;
	}

	.preview-card {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.85rem;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	.preview-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.85rem 1.15rem;
		background: var(--color-surface-high);
		border-bottom: 1px solid var(--color-line);
	}

	.file-name {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.85rem;
		font-weight: 600;
		color: var(--color-heading);
	}

	.copy-small-btn {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		color: var(--color-heading);
		padding: 0.25rem 0.65rem;
		border-radius: 0.35rem;
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
	}

	.copy-small-btn:hover {
		border-color: var(--color-outline);
	}

	.yaml-code {
		padding: 1.25rem;
		margin: 0;
		font-size: 0.825rem;
		line-height: 1.5;
		color: var(--color-heading);
		overflow-x: auto;
		background: var(--color-bg);
		max-height: 480px;
	}

	.preview-footer {
		padding: 0.85rem 1.15rem;
		background: var(--color-surface-high);
		border-top: 1px solid var(--color-line);
		font-size: 0.775rem;
		color: var(--color-muted);
	}
</style>
