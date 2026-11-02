<script lang="ts">
	import { onMount } from 'svelte';
	import type { SkillSummary, FeedbackRecord } from '../types';

	let activeSection = $state<'memory' | 'skills'>('memory');
	let feedbackList = $state<FeedbackRecord[]>([]);
	let skillsList = $state<SkillSummary[]>([]);
	let skillsDir = $state<string>('');
	let loadingFeedback = $state(true);
	let loadingSkills = $state(true);
	let skillSearch = $state('');
	let deletingId = $state<number | null>(null);

	onMount(() => {
		loadFeedback();
		loadSkills();
	});

	async function loadFeedback() {
		loadingFeedback = true;
		try {
			const res = await fetch('/api/feedback');
			if (res.ok) {
				const data = await res.json();
				feedbackList = data.feedback || [];
			}
		} catch (e) {
			console.error('Failed to load feedback:', e);
		} finally {
			loadingFeedback = false;
		}
	}

	async function loadSkills() {
		loadingSkills = true;
		try {
			const res = await fetch('/api/skills');
			if (res.ok) {
				const data = await res.json();
				skillsList = data.skills || [];
				skillsDir = data.skills_dir || '';
			}
		} catch (e) {
			console.error('Failed to load skills:', e);
		} finally {
			loadingSkills = false;
		}
	}

	async function deleteConstraint(id: number) {
		deletingId = id;
		try {
			const res = await fetch('/api/feedback/delete', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ id })
			});
			if (res.ok) {
				feedbackList = feedbackList.filter((f) => f.id !== id);
			}
		} catch (e) {
			console.error('Failed to delete constraint:', e);
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
	<!-- Hero / Header -->
	<div class="brain-header">
		<div>
			<h2>🧠 Revue Brain & Fleet Skills</h2>
			<p class="subtitle">
				Episodic negative constraints and autonomous domain skills driving the two-pass code review engine
			</p>
		</div>

		<!-- Section Tabs -->
		<div class="tab-pill-group">
			<button
				class="tab-pill"
				class:active={activeSection === 'memory'}
				onclick={() => (activeSection = 'memory')}
			>
				<span>🧠</span>
				<span>Episodic Memory ({feedbackList.length})</span>
			</button>
			<button
				class="tab-pill"
				class:active={activeSection === 'skills'}
				onclick={() => (activeSection = 'skills')}
			>
				<span>🛠️</span>
				<span>Fleet Skills ({skillsList.length})</span>
			</button>
		</div>
	</div>

	<!-- Section 1: Episodic Memory -->
	{#if activeSection === 'memory'}
		<div class="memory-section">
			<div class="section-banner">
				<div class="banner-icon">💡</div>
				<div class="banner-content">
					<h4>How Revue Learns</h4>
					<p>
						When an engineer dismisses a finding as a <strong>False Alarm</strong> on the dashboard, Revue records the repository, file pattern, and dismissal context in the persistent SQLite/PostgreSQL store. During Pass 2, the Judge LLM loads these records as strict negative constraints, suppressing repeat hallucinations for future PRs in the same repository.
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
						When you review PRs on the Triage board and click <strong>"👎 False Alarm"</strong> on any finding card, Revue will catalog it here and ensure the Pass 2 Judge never repeats it.
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
									class="forget-btn"
									disabled={deletingId === item.id}
									onclick={() => deleteConstraint(item.id)}
									title="Un-dismiss this false alarm so Revue will re-evaluate it in future reviews"
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

	<!-- Section 2: Fleet Skills Directory -->
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
					type="text"
					placeholder="Filter skills by name, trigger (e.g. .svelte, .nix), or topic..."
					class="skill-search"
					bind:value={skillSearch}
				/>
			</div>

			{#if loadingSkills}
				<div class="loading-state">Indexing fleet skills...</div>
			{:else if filteredSkills.length === 0}
				<div class="empty-skills">No fleet skills matching "{skillSearch}".</div>
			{:else}
				<div class="skills-grid">
					{#each filteredSkills as skill}
						<div class="skill-card">
							<div class="skill-card-top">
								<span class="skill-name">{skill.name}</span>
								<span class="stage-tag">Pass 1 & Pass 2</span>
							</div>

							<p class="skill-desc">
								{skill.description === '>-' || skill.description === '>' ? 'Comprehensive fleet conventions and rules.' : skill.description}
							</p>

							{#if skill.triggers && skill.triggers.length > 0}
								<div class="triggers-wrap">
									<span class="triggers-label">Path Triggers:</span>
									<div class="trigger-pills">
										{#each skill.triggers as trigger}
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
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
	}

	.brain-header h2 {
		font-size: 1.65rem;
		font-weight: 700;
		color: #f0f6fc;
		margin: 0 0 0.4rem 0;
	}

	.subtitle {
		font-size: 0.9rem;
		color: #8b949e;
		margin: 0;
	}

	.tab-pill-group {
		display: flex;
		background: #0d1117;
		padding: 0.25rem;
		border-radius: 0.65rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
	}

	.tab-pill {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.5rem 1rem;
		border-radius: 0.45rem;
		background: transparent;
		border: none;
		color: #8b949e;
		font-size: 0.85rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.tab-pill.active {
		background: #21262d;
		color: #f0f6fc;
	}

	/* Section Banner */
	.section-banner {
		display: flex;
		align-items: flex-start;
		gap: 1.25rem;
		padding: 1.25rem 1.5rem;
		background: rgba(139, 92, 246, 0.08);
		border: 1px solid rgba(139, 92, 246, 0.25);
		border-radius: 0.85rem;
		margin-bottom: 1.5rem;
	}

	.banner-icon {
		font-size: 1.75rem;
	}

	.banner-content h4 {
		font-size: 1rem;
		font-weight: 700;
		color: #d8b4fe;
		margin: 0 0 0.35rem 0;
	}

	.banner-content p {
		font-size: 0.875rem;
		color: #c9d1d9;
		line-height: 1.5;
		margin: 0;
	}

	/* Memory Grid */
	.memory-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
		gap: 1.25rem;
	}

	.memory-card {
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
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
	}

	.repo-badge {
		font-size: 0.75rem;
		font-weight: 700;
		color: #58a6ff;
		background: rgba(56, 139, 253, 0.12);
		border: 1px solid rgba(56, 139, 253, 0.25);
		padding: 0.2rem 0.55rem;
		border-radius: 9999px;
	}

	.active-badge {
		display: flex;
		align-items: center;
		gap: 0.35rem;
		font-size: 0.75rem;
		font-weight: 600;
		color: #a78bfa;
		background: rgba(167, 139, 250, 0.1);
		padding: 0.2rem 0.5rem;
		border-radius: 9999px;
	}

	.pulse-dot {
		width: 6px;
		height: 6px;
		border-radius: 9999px;
		background: #a78bfa;
		box-shadow: 0 0 6px #a78bfa;
	}

	.memory-file code {
		font-size: 0.8rem;
		color: #e6edf3;
		background: #0d1117;
		padding: 0.25rem 0.5rem;
		border-radius: 4px;
		border: 1px solid rgba(255, 255, 255, 0.06);
	}

	.memory-title {
		font-size: 0.95rem;
		font-weight: 600;
		color: #f0f6fc;
		margin: 0;
	}

	.reason-box {
		background: #0d1117;
		border: 1px solid rgba(255, 255, 255, 0.06);
		border-radius: 6px;
		padding: 0.65rem 0.85rem;
	}

	.reason-label {
		font-size: 0.725rem;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		color: #8b949e;
		display: block;
		margin-bottom: 0.25rem;
	}

	.reason-text {
		font-size: 0.85rem;
		font-style: italic;
		color: #c9d1d9;
		margin: 0;
	}

	.memory-footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding-top: 0.5rem;
		border-top: 1px solid rgba(255, 255, 255, 0.06);
		margin-top: auto;
	}

	.timestamp {
		font-size: 0.75rem;
		color: #8b949e;
	}

	.forget-btn {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		background: transparent;
		border: 1px solid rgba(239, 68, 68, 0.3);
		color: #fca5a5;
		padding: 0.3rem 0.65rem;
		border-radius: 5px;
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
		transition: all 0.15s ease;
	}

	.forget-btn:hover:not(:disabled) {
		background: rgba(239, 68, 68, 0.15);
		border-color: #ef4444;
		color: #ffffff;
	}

	/* Skills Section */
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
		color: #f0f6fc;
		margin: 0 0 0.25rem 0;
	}

	.dir-hint {
		font-size: 0.75rem;
		color: #8b949e;
	}

	.dir-hint code {
		color: #58a6ff;
	}

	.skill-search {
		background: #0d1117;
		border: 1px solid #30363d;
		border-radius: 0.45rem;
		padding: 0.5rem 0.85rem;
		color: #f0f6fc;
		font-size: 0.85rem;
		outline: none;
		min-width: 300px;
	}

	.skill-search:focus {
		border-color: #8b5cf6;
	}

	.skills-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
		gap: 1.25rem;
	}

	.skill-card {
		background: #161b22;
		border: 1px solid rgba(255, 255, 255, 0.08);
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
	}

	.skill-name {
		font-size: 1rem;
		font-weight: 700;
		color: #f0f6fc;
	}

	.stage-tag {
		font-size: 0.7rem;
		font-weight: 600;
		color: #10b981;
		background: rgba(16, 185, 129, 0.1);
		border: 1px solid rgba(16, 185, 129, 0.25);
		padding: 0.15rem 0.45rem;
		border-radius: 9999px;
	}

	.skill-desc {
		font-size: 0.85rem;
		color: #8b949e;
		line-height: 1.45;
		margin: 0;
		flex: 1;
	}

	.triggers-wrap {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
		padding-top: 0.5rem;
		border-top: 1px solid rgba(255, 255, 255, 0.06);
	}

	.triggers-label {
		font-size: 0.7rem;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		color: #6e7681;
	}

	.trigger-pills {
		display: flex;
		flex-wrap: wrap;
		gap: 0.35rem;
	}

	.trigger-pill {
		font-size: 0.75rem;
		color: #d8b4fe;
		background: rgba(168, 85, 247, 0.1);
		border: 1px solid rgba(168, 85, 247, 0.25);
		padding: 0.15rem 0.45rem;
		border-radius: 4px;
	}

	.loading-state,
	.empty-memory,
	.empty-skills {
		padding: 3rem;
		text-align: center;
		color: #8b949e;
		background: #12161f;
		border-radius: 0.85rem;
		border: 1px solid rgba(255, 255, 255, 0.06);
	}

	.empty-icon {
		font-size: 2.5rem;
		margin-bottom: 0.5rem;
	}
</style>
