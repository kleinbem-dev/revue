<script lang="ts">
	import { onMount } from 'svelte';
	import { apiFetch, type SkillSummary, type FeedbackRecord } from '#lib';

	let activeSection = $state<'memory' | 'skills'>('memory');
	let feedbackList = $state<FeedbackRecord[]>([]);
	let skillsList = $state<SkillSummary[]>([]);
	let skillsDir = $state<string>('');
	let loadingFeedback = $state(true);
	let loadingSkills = $state(true);
	let errorMessage = $state<string | null>(null);
	let skillSearch = $state('');
	let deletingId = $state<number | null>(null);

	onMount(() => {
		loadFeedback();
		loadSkills();
	});

	async function loadFeedback() {
		loadingFeedback = true;
		try {
			const res = await apiFetch('/api/feedback');
			if (res.ok) {
				const data = await res.json();
				feedbackList = data.feedback || [];
			} else if (res.status === 401 || res.status === 403) {
				errorMessage = 'API authentication required. Configure your REVUE_API_KEY in the top bar.';
			} else {
				errorMessage = `Failed to load episodic memory (HTTP ${res.status}).`;
			}
		} catch (e) {
			errorMessage = e instanceof Error ? e.message : 'Failed to connect to Revue daemon.';
		} finally {
			loadingFeedback = false;
		}
	}

	async function loadSkills() {
		loadingSkills = true;
		try {
			const res = await apiFetch('/api/skills');
			if (res.ok) {
				const data = await res.json();
				skillsList = data.skills || [];
				skillsDir = data.skills_dir || '';
			} else if (res.status === 401 || res.status === 403) {
				errorMessage = 'API authentication required. Configure your REVUE_API_KEY in the top bar.';
			} else {
				errorMessage = `Failed to load skills directory (HTTP ${res.status}).`;
			}
		} catch (e) {
			errorMessage = e instanceof Error ? e.message : 'Failed to connect to Revue daemon.';
		} finally {
			loadingSkills = false;
		}
	}

	async function deleteConstraint(id: number) {
		deletingId = id;
		errorMessage = null;
		try {
			const res = await apiFetch('/api/feedback/delete', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ id })
			});
			if (res.ok) {
				feedbackList = feedbackList.filter((f) => f.id !== id);
			} else {
				errorMessage = `Failed to delete constraint (HTTP ${res.status}).`;
			}
		} catch (e) {
			errorMessage = e instanceof Error ? e.message : 'Network error deleting constraint.';
		} finally {
			deletingId = null;
		}
	}

	const filteredSkills = $derived(
		skillSearch.trim() === ''
			? skillsList
			: skillsList.filter(
					(s) =>
						s.name.toLowerCase().includes(skillSearch.toLowerCase()) ||
						s.description.toLowerCase().includes(skillSearch.toLowerCase()) ||
						s.triggers.some((t) => t.toLowerCase().includes(skillSearch.toLowerCase()))
				)
	);
</script>

<div class="brain-view">
	<div class="brain-header">
		<div>
			<h2>🧠 Revue Brain & Skills Registry</h2>
			<p class="subtitle">
				Episodic negative constraints and domain skills driving the two-pass code review engine
			</p>
		</div>

		<div class="tab-pill-group" role="tablist" aria-label="Brain sections">
			<button
				type="button"
				role="tab"
				aria-selected={activeSection === 'memory'}
				class="tab-pill"
				class:active={activeSection === 'memory'}
				onclick={() => (activeSection = 'memory')}
			>
				<span>🧠</span>
				<span>Episodic Memory ({feedbackList.length})</span>
			</button>
			<button
				type="button"
				role="tab"
				aria-selected={activeSection === 'skills'}
				class="tab-pill"
				class:active={activeSection === 'skills'}
				onclick={() => (activeSection = 'skills')}
			>
				<span>🛠️</span>
				<span>Skills ({skillsList.length})</span>
			</button>
		</div>
	</div>

	{#if errorMessage}
		<div class="error-banner" role="alert">
			<span>⚠️ {errorMessage}</span>
		</div>
	{/if}

	{#if activeSection === 'memory'}
		<div class="memory-section">
			<div class="section-banner">
				<div class="banner-icon">💡</div>
				<div class="banner-content">
					<h4>How Revue Learns</h4>
					<p>
						When an engineer dismisses a finding as a <strong>False Alarm</strong> on the dashboard,
						Revue records the repository, file pattern, and dismissal context in its persistent
						SQLite/PostgreSQL store. During Pass 2, the Judge loads these records as negative
						constraints, suppressing repeat false positives for future PRs in the same repository.
					</p>
				</div>
			</div>

			{#if loadingFeedback}
				<div class="loading-state">Loading episodic memory constraints...</div>
			{:else if feedbackList.length === 0}
				<div class="empty-memory">
					<div class="empty-icon">🌱</div>
					<h3>No False Alarm Constraints Recorded Yet</h3>
					<p>
						When you review PRs on the Triage board and click <strong>"👎 False Alarm"</strong> on
						any finding card, Revue will catalog it here and ensure the Pass 2 Judge suppresses
						repeats.
					</p>
				</div>
			{:else}
				<div class="memory-grid">
					{#each feedbackList as item (item.id)}
						<div class="memory-card">
							<div class="memory-card-top">
								<span class="repo-badge">{item.repo}</span>
								<span class="active-badge">
									<span class="pulse-dot"></span>
									Active in Judge Prompt
								</span>
							</div>

							<div class="memory-file">
								<code>{item.file_path}</code>
							</div>

							<h4 class="memory-title">{item.issue_title}</h4>

							{#if item.reason}
								<div class="reason-box">
									<span class="reason-label">Engineer Feedback:</span>
									<p class="reason-text">"{item.reason}"</p>
								</div>
							{/if}

							<div class="memory-footer">
								<span class="timestamp">Learned {item.created_at}</span>
								<button
									type="button"
									class="forget-btn"
									disabled={deletingId === item.id}
									onclick={() => deleteConstraint(item.id)}
									title="Remove this negative constraint so Revue re-evaluates it in future reviews"
								>
									{#if deletingId === item.id}
										<span>⏳</span> Forgetting...
									{:else}
										<span>🗑️</span> Forget Constraint
									{/if}
								</button>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}

	{#if activeSection === 'skills'}
		<div class="skills-section">
			<div class="skills-controls">
				<div class="skills-meta">
					<h4>Discovered Skills ({filteredSkills.length})</h4>
					{#if skillsDir}
						<span class="dir-hint">Source: <code>{skillsDir}</code></span>
					{/if}
				</div>

				<input
					type="search"
					placeholder="Filter skills by name, trigger (e.g. .svelte, .nix), or topic..."
					aria-label="Filter skills"
					class="skill-search"
					bind:value={skillSearch}
				/>
			</div>

			{#if loadingSkills}
				<div class="loading-state">Indexing skills...</div>
			{:else if filteredSkills.length === 0}
				<div class="empty-skills">No skills matching "{skillSearch}".</div>
			{:else}
				<div class="skills-grid">
					{#each filteredSkills as skill (skill.name)}
						<div class="skill-card">
							<div class="skill-card-top">
								<span class="skill-name">{skill.name}</span>
								<span class="stage-tag">Pass 1 & Pass 2</span>
							</div>

							<p class="skill-desc">
								{skill.description === '>-' || skill.description === '>'
									? 'Domain review conventions and rules.'
									: skill.description}
							</p>

							{#if skill.triggers && skill.triggers.length > 0}
								<div class="triggers-wrap">
									<span class="triggers-label">Path Triggers:</span>
									<div class="trigger-pills">
										{#each skill.triggers as trigger (trigger)}
											<code class="trigger-pill">{trigger}</code>
										{/each}
									</div>
								</div>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>

<style>
	.brain-view {
		display: flex;
		flex-direction: column;
		gap: 2rem;
	}

	.brain-header {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 1.5rem;
		padding-bottom: 1.5rem;
		border-bottom: 1px solid var(--color-line);
	}

	.brain-header h2 {
		font-size: 1.65rem;
		font-weight: 700;
		color: var(--color-heading);
		margin: 0 0 0.4rem 0;
	}

	.subtitle {
		font-size: 0.9rem;
		color: var(--color-muted);
		margin: 0;
	}

	.tab-pill-group {
		display: flex;
		background: var(--color-surface);
		padding: 0.25rem;
		border-radius: 0.65rem;
		border: 1px solid var(--color-line);
	}

	.tab-pill {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.5rem 1rem;
		border-radius: 0.45rem;
		background: transparent;
		border: none;
		color: var(--color-muted);
		font-size: 0.85rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.tab-pill:hover {
		color: var(--color-heading);
	}

	.tab-pill.active {
		background: var(--color-surface-high);
		color: var(--color-heading);
		box-shadow: var(--shadow-elevation-1);
	}

	.error-banner {
		padding: 0.85rem 1.1rem;
		border-radius: 0.65rem;
		background: var(--color-status-danger-soft);
		border: 1px solid var(--color-status-danger-border);
		color: var(--color-status-danger);
		font-size: 0.85rem;
	}

	.section-banner {
		display: flex;
		align-items: flex-start;
		gap: 1.25rem;
		padding: 1.25rem 1.5rem;
		background: var(--color-status-ai-soft);
		border: 1px solid var(--color-status-ai-border);
		border-radius: 0.85rem;
		margin-bottom: 1.5rem;
	}

	.banner-icon {
		font-size: 1.75rem;
	}

	.banner-content h4 {
		font-size: 1rem;
		font-weight: 700;
		color: var(--color-status-ai);
		margin: 0 0 0.35rem 0;
	}

	.banner-content p {
		font-size: 0.875rem;
		color: var(--color-fg);
		line-height: 1.5;
		margin: 0;
	}

	.memory-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
		gap: 1.25rem;
	}

	.memory-card {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.75rem;
		padding: 1.25rem;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.memory-card-top {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.repo-badge {
		font-size: 0.75rem;
		font-weight: 700;
		color: var(--color-status-info);
		background: var(--color-status-info-soft);
		border: 1px solid var(--color-status-info-border);
		padding: 0.2rem 0.55rem;
		border-radius: 9999px;
	}

	.active-badge {
		display: flex;
		align-items: center;
		gap: 0.35rem;
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--color-status-ai);
		background: var(--color-status-ai-soft);
		padding: 0.2rem 0.5rem;
		border-radius: 9999px;
	}

	.pulse-dot {
		width: 6px;
		height: 6px;
		border-radius: 9999px;
		background: var(--color-status-ai);
	}

	.memory-file code {
		font-size: 0.8rem;
		font-family: var(--font-mono);
		color: var(--color-heading);
		background: var(--color-bg);
		padding: 0.25rem 0.5rem;
		border-radius: 4px;
		border: 1px solid var(--color-line);
	}

	.memory-title {
		font-size: 0.95rem;
		font-weight: 600;
		color: var(--color-heading);
		margin: 0;
	}

	.reason-box {
		background: var(--color-bg);
		border: 1px solid var(--color-line);
		border-radius: 6px;
		padding: 0.65rem 0.85rem;
	}

	.reason-label {
		font-size: 0.725rem;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		color: var(--color-muted);
		display: block;
		margin-bottom: 0.25rem;
	}

	.reason-text {
		font-size: 0.85rem;
		font-style: italic;
		color: var(--color-fg);
		margin: 0;
	}

	.memory-footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding-top: 0.5rem;
		border-top: 1px solid var(--color-line);
		margin-top: auto;
	}

	.timestamp {
		font-size: 0.75rem;
		color: var(--color-muted);
	}

	.forget-btn {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		background: var(--color-status-danger-soft);
		border: 1px solid var(--color-status-danger-border);
		color: var(--color-status-danger);
		padding: 0.3rem 0.65rem;
		border-radius: 5px;
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.forget-btn:hover:not(:disabled) {
		border-color: var(--color-status-danger);
	}

	.skills-controls {
		display: flex;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 1rem;
		margin-bottom: 1.5rem;
	}

	.skills-meta h4 {
		font-size: 1.1rem;
		font-weight: 700;
		color: var(--color-heading);
		margin: 0 0 0.25rem 0;
	}

	.dir-hint {
		font-size: 0.75rem;
		color: var(--color-muted);
	}

	.dir-hint code {
		font-family: var(--font-mono);
		color: var(--color-status-info);
	}

	.skill-search {
		background: var(--color-surface);
		border: 1px solid var(--color-outline);
		border-radius: 0.45rem;
		padding: 0.5rem 0.85rem;
		color: var(--color-heading);
		font-size: 0.85rem;
		outline: none;
		min-width: 280px;
	}

	.skill-search:focus {
		border-color: var(--color-accent);
	}

	.skills-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
		gap: 1.25rem;
	}

	.skill-card {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.75rem;
		padding: 1.25rem;
		display: flex;
		flex-direction: column;
		gap: 0.65rem;
	}

	.skill-card-top {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.5rem;
	}

	.skill-name {
		font-size: 1rem;
		font-weight: 700;
		color: var(--color-heading);
	}

	.stage-tag {
		font-size: 0.7rem;
		font-weight: 600;
		color: var(--color-status-success);
		background: var(--color-status-success-soft);
		border: 1px solid var(--color-status-success-border);
		padding: 0.15rem 0.45rem;
		border-radius: 9999px;
	}

	.skill-desc {
		font-size: 0.85rem;
		color: var(--color-muted);
		line-height: 1.45;
		margin: 0;
		flex: 1;
	}

	.triggers-wrap {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
		padding-top: 0.5rem;
		border-top: 1px solid var(--color-line);
	}

	.triggers-label {
		font-size: 0.7rem;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		color: var(--color-muted);
	}

	.trigger-pills {
		display: flex;
		flex-wrap: wrap;
		gap: 0.35rem;
	}

	.trigger-pill {
		font-size: 0.75rem;
		font-family: var(--font-mono);
		color: var(--color-status-ai);
		background: var(--color-status-ai-soft);
		border: 1px solid var(--color-status-ai-border);
		padding: 0.15rem 0.45rem;
		border-radius: 4px;
	}

	.loading-state,
	.empty-memory,
	.empty-skills {
		padding: 3rem;
		text-align: center;
		color: var(--color-muted);
		background: var(--color-surface);
		border-radius: 0.85rem;
		border: 1px solid var(--color-line);
	}

	.empty-memory h3 {
		color: var(--color-heading);
	}

	.empty-icon {
		font-size: 2.5rem;
		margin-bottom: 0.5rem;
	}
</style>
