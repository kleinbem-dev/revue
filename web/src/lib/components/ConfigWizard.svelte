<script lang="ts">
	import type { WizardConfig } from '../types';

	let config = $state<WizardConfig>({
		profile: 'chill',
		max_merge_risk: 'medium',
		high_level_summary: true,
		anti_slop: true,
		ignore_lockfiles: true,
		ignore_vendor: true,
		ignore_generated: true,
		custom_guidelines: 'Enforce zero-trust network boundaries.\nNever hardcode credentials or secrets in plain text.\nRequire signed commits for production branches.'
	});

	let copied = $state(false);

	const generatedYaml = $derived.by(() => {
		const toneByProfile: Record<string, string> = {
			chill: 'Short and direct. Lead with the problem and the fix. No praise, no filler.',
			quiet: 'Concise and minimal. Only report actionable errors and critical issues. No praise.',
			assertive: 'Strict and thorough. Flag potential regressions, security edge cases, and style issues.'
		};

		const severityByProfile: Record<string, string> = {
			chill: 'warning',
			quiet: 'error',
			assertive: 'info'
		};

		const lines = [
			'# yaml-language-server: $schema=https://revue.kleinbem.dev/schema.json',
			'# Revue AI Code Review Configuration',
			'version: 1',
			'',
			'review:',
			`  tone: ${JSON.stringify(toneByProfile[config.profile] ?? toneByProfile.chill)}`,
			`  min_severity: "${severityByProfile[config.profile] ?? 'warning'}"`,
			`  summary: ${config.high_level_summary}`,
			`  two_pass: ${config.anti_slop}`,
			'  semgrep: true'
		];

		const ignores: string[] = [];
		if (config.ignore_lockfiles) {
			ignores.push('!**/*.lock', '!**/flake.lock', '!**/Cargo.lock', '!**/package-lock.json');
		}
		if (config.ignore_vendor) {
			ignores.push('!**/vendor/**', '!**/node_modules/**');
		}
		if (config.ignore_generated) {
			ignores.push('!**/*.generated.*', '!**/dist/**', '!**/target/**');
		}

		if (ignores.length > 0) {
			lines.push('  ignore:');
			for (const ig of ignores) {
				lines.push(`    - "${ig}"`);
			}
		}

		if (config.custom_guidelines.trim()) {
			lines.push('  rules:');
			lines.push('    - path: "**/*"');
			const guidelineLines = config.custom_guidelines
				.split('\n')
				.map((l) => l.trim())
				.filter(Boolean);
			if (guidelineLines.length === 1) {
				lines.push(`      instructions: ${JSON.stringify(guidelineLines[0])}`);
			} else if (guidelineLines.length > 1) {
				lines.push('      instructions: |');
				for (const gl of guidelineLines) {
					lines.push(`        ${gl}`);
				}
			}
		}

		return lines.join('\n');
	});

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
	<!-- Hero Header -->
	<div class="wizard-header">
		<div class="header-left">
			<span class="sparkle">🥕</span>
			<div>
				<h2>Rules & Configuration Wizard</h2>
				<p class="subtitle">
					Configure AI review strictness, path filters, and guidelines for your repositories.
				</p>
			</div>
		</div>
		<div class="header-actions">
			<button class="btn primary" onclick={copyYaml}>
				{#if copied}
					<span class="check">✓</span> Copied .revue.yaml!
				{:else}
					<span>📋</span> Copy .revue.yaml
				{/if}
			</button>
			<button class="btn outline" onclick={downloadYaml}>
				<span>⬇</span> Download
			</button>
		</div>
	</div>

	<!-- Wizard Grid -->
	<div class="wizard-grid">
		<!-- Left: Controls Form -->
		<div class="controls-panel">
			<!-- Tone & Strictness -->
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
							<div class="p-desc">Deep and thorough. Flags style idioms, edge cases, refactoring suggestions.</div>
						</div>
					</label>
				</div>
			</div>

			<!-- Workflow & Approvals -->
			<div class="card">
				<div class="card-title">
					<span class="badge-num">2</span>
					<div>
						<h3>Review Automation & Governance</h3>
						<span class="hint">Control PR summary comments and automated merge gates</span>
					</div>
				</div>

				<div class="toggles-list">
					<label class="toggle-row">
						<div>
							<div class="t-name">High-Level Executive Summary</div>
							<div class="t-sub">Post a 2-sentence impact summary and risk assessment on PRs</div>
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
				</div>
			</div>

			<!-- Path Inclusions & Filters -->
			<div class="card">
				<div class="card-title">
					<span class="badge-num">3</span>
					<div>
						<h3>File & Path Filters</h3>
						<span class="hint">Exclude noise files from AI context window to save tokens</span>
					</div>
				</div>

				<div class="toggles-list">
					<label class="toggle-row">
						<div>
							<div class="t-name">Ignore Lockfiles</div>
							<div class="t-sub"><code>!**/*.lock</code>, <code>flake.lock</code>, <code>Cargo.lock</code></div>
						</div>
						<input type="checkbox" bind:checked={config.ignore_lockfiles} />
					</label>

					<label class="toggle-row">
						<div>
							<div class="t-name">Ignore Vendored Dependencies</div>
							<div class="t-sub"><code>!**/vendor/**</code>, <code>!**/node_modules/**</code></div>
						</div>
						<input type="checkbox" bind:checked={config.ignore_vendor} />
					</label>

					<label class="toggle-row">
						<div>
							<div class="t-name">Ignore Generated Artifacts</div>
							<div class="t-sub"><code>!**/target/**</code>, <code>!**/dist/**</code>, <code>*.generated.*</code></div>
						</div>
						<input type="checkbox" bind:checked={config.ignore_generated} />
					</label>
				</div>
			</div>

			<!-- Custom Instructions -->
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
					placeholder="Enter rules, one per line..."
					bind:value={config.custom_guidelines}
				></textarea>
			</div>
		</div>

		<!-- Right: Live Generated YAML Preview -->
		<div class="preview-panel">
			<div class="preview-card">
				<div class="preview-header">
					<div class="file-name">
						<span>📄</span>
						<code>.revue.yaml</code>
					</div>
					<button class="copy-small-btn" onclick={copyYaml}>
						{#if copied}Copied!{:else}Copy YAML{/if}
					</button>
				</div>
				<pre class="yaml-code"><code>{generatedYaml}</code></pre>
				<div class="preview-footer">
					<span>💡 Place this file in your repository root to apply custom guidelines.</span>
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
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
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
		color: #f0f6fc;
		margin: 0;
	}

	.subtitle {
		font-size: 0.875rem;
		color: #8b949e;
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
		background: #f97316;
		color: white;
	}

	.btn.primary:hover {
		background: #ea580c;
	}

	.btn.outline {
		background: #21262d;
		border-color: rgba(255, 255, 255, 0.1);
		color: #f0f6fc;
	}

	.btn.outline:hover {
		background: #30363d;
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
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
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
		background: rgba(249, 115, 22, 0.15);
		color: #f97316;
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
		color: #f0f6fc;
		margin: 0;
	}

	.hint {
		font-size: 0.8rem;
		color: #8b949e;
	}

	/* Profiles */
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
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.65rem;
		background: #0d1117;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.profile-card:hover {
		border-color: rgba(249, 115, 22, 0.3);
	}

	.profile-card.selected {
		border-color: #f97316;
		background: rgba(249, 115, 22, 0.05);
	}

	.profile-card input {
		margin-top: 3px;
		accent-color: #f97316;
	}

	.p-title {
		font-size: 0.9rem;
		font-weight: 600;
		color: #f0f6fc;
	}

	.p-desc {
		font-size: 0.8rem;
		color: #8b949e;
		margin-top: 0.15rem;
	}

	/* Toggles */
	.toggles-list {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.toggle-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.75rem 1rem;
		background: #0d1117;
		border: 1px solid rgba(255, 255, 255, 0.06);
		border-radius: 0.65rem;
		cursor: pointer;
	}

	.toggle-row input {
		width: 18px;
		height: 18px;
		accent-color: #f97316;
		cursor: pointer;
	}

	.t-name {
		font-size: 0.875rem;
		font-weight: 600;
		color: #f0f6fc;
	}

	.t-sub {
		font-size: 0.775rem;
		color: #8b949e;
		margin-top: 0.15rem;
	}

	.t-sub code {
		background: rgba(255, 255, 255, 0.06);
		padding: 0.1rem 0.35rem;
		border-radius: 0.25rem;
		color: #58a6ff;
	}

	/* Guidelines Textarea */
	.guidelines-input {
		width: 100%;
		background: #0d1117;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.5rem;
		padding: 0.85rem;
		color: #f0f6fc;
		font-family: ui-monospace, monospace;
		font-size: 0.85rem;
		line-height: 1.5;
		resize: vertical;
		outline: none;
		box-sizing: border-box;
	}

	.guidelines-input:focus {
		border-color: #f97316;
	}

	/* Preview Panel */
	.preview-panel {
		position: sticky;
		top: 5rem;
	}

	.preview-card {
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
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
		background: #0d1117;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
	}

	.file-name {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.85rem;
		font-weight: 600;
		color: #f0f6fc;
	}

	.copy-small-btn {
		background: #21262d;
		border: 1px solid rgba(255, 255, 255, 0.1);
		color: #f0f6fc;
		padding: 0.25rem 0.65rem;
		border-radius: 0.35rem;
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
	}

	.copy-small-btn:hover {
		background: #30363d;
	}

	.yaml-code {
		padding: 1.25rem;
		margin: 0;
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.825rem;
		line-height: 1.5;
		color: #e6edf3;
		overflow-x: auto;
		background: #090d13;
		max-height: 480px;
	}

	.preview-footer {
		padding: 0.85rem 1.15rem;
		background: #0d1117;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
		font-size: 0.775rem;
		color: #8b949e;
	}
</style>
