<script lang="ts">
	import { page } from '$app/state';
	import FontSizeToggle from '@kleinbem/web-config/font-size/toggle';
	import ThemeToggle from '@kleinbem/web-config/theme/toggle';

	let {
		version = '0.1.0',
		forgeUrl = null,
		hasApiKey = false,
		onToggleApiKey
	}: {
		version?: string;
		forgeUrl?: string | null;
		hasApiKey?: boolean;
		onToggleApiKey?: () => void;
	} = $props();

	const currentPath = $derived(page.url.pathname);
</script>

<header class="header">
	<div class="logo-area">
		<span class="icon" aria-hidden="true">🎭</span>
		<div class="brand">
			<h1>Revue</h1>
			<span class="badge">v{version}</span>
		</div>
		<span class="tagline">Self-hosted, forge-agnostic AI code review</span>
	</div>

	<nav class="nav-tabs" aria-label="Primary navigation">
		<a
			href="/"
			class="nav-tab"
			class:active={currentPath === '/'}
			data-sveltekit-preload-data="hover"
		>
			<span class="tab-icon" aria-hidden="true">📊</span>
			<span>Triage & Reviews</span>
		</a>

		<a
			href="/wizard"
			class="nav-tab"
			class:active={currentPath.startsWith('/wizard')}
			data-sveltekit-preload-data="hover"
		>
			<span class="tab-icon" aria-hidden="true">⚙️</span>
			<span>Rules & Config Wizard</span>
		</a>

		<a
			href="/brain"
			class="nav-tab"
			class:active={currentPath.startsWith('/brain')}
			data-sveltekit-preload-data="hover"
		>
			<span class="tab-icon" aria-hidden="true">🧠</span>
			<span>Brain & Skills</span>
		</a>
	</nav>

	<div class="header-actions">
		<a
			href="https://github.com/kleinbem-dev/revue"
			target="_blank"
			rel="noreferrer"
			class="btn btn-outline"
		>
			<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
				<path
					d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"
				/>
			</svg>
			GitHub
		</a>

		{#if forgeUrl}
			<a href={forgeUrl} target="_blank" rel="noreferrer" class="btn btn-outline forgejo">
				<span class="dot" aria-hidden="true"></span>
				Forge
			</a>
		{/if}

		{#if onToggleApiKey}
			<button
				type="button"
				class="btn btn-outline key-btn"
				class:configured={hasApiKey}
				onclick={onToggleApiKey}
				title={hasApiKey ? 'Update or clear API key' : 'Configure API key'}
			>
				🔑 {hasApiKey ? 'Key Set' : 'API Key'}
			</button>
		{/if}

		<div class="toggle-group">
			<FontSizeToggle />
			<ThemeToggle />
		</div>
	</div>
</header>

<style>
	.toggle-group {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		margin-left: 0.25rem;
	}

	.header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1rem 2rem;
		border-bottom: 1px solid var(--color-line);
		background: color-mix(in srgb, var(--color-bg) 88%, transparent);
		backdrop-filter: blur(12px);
		position: sticky;
		top: 0;
		z-index: 50;
		flex-wrap: wrap;
		gap: 1rem;
	}

	.logo-area {
		display: flex;
		align-items: center;
		gap: 0.85rem;
	}

	.icon {
		font-size: 1.85rem;
	}

	.brand {
		display: flex;
		align-items: baseline;
		gap: 0.5rem;
	}

	h1 {
		font-size: 1.45rem;
		font-weight: 700;
		letter-spacing: -0.025em;
		color: var(--color-heading);
		margin: 0;
	}

	.badge {
		font-size: 0.7rem;
		font-weight: 600;
		color: var(--color-muted);
		background: var(--color-surface);
		padding: 0.15rem 0.5rem;
		border-radius: 9999px;
		border: 1px solid var(--color-line);
	}

	.tagline {
		color: var(--color-muted);
		font-size: 0.85rem;
		padding-left: 0.75rem;
		border-left: 1px solid var(--color-line);
	}

	.nav-tabs {
		display: flex;
		align-items: center;
		gap: 0.35rem;
		background: var(--color-surface);
		padding: 0.25rem;
		border-radius: 0.6rem;
		border: 1px solid var(--color-line);
	}

	.nav-tab {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.45rem 0.95rem;
		border-radius: 0.45rem;
		font-size: 0.825rem;
		font-weight: 600;
		color: var(--color-muted);
		background: transparent;
		border: none;
		cursor: pointer;
		text-decoration: none;
		transition: all 0.15s ease;
	}

	.nav-tab:hover {
		color: var(--color-heading);
	}

	.nav-tab.active {
		background: var(--color-surface-high);
		color: var(--color-accent-soft-fg);
		box-shadow: var(--shadow-elevation-1);
	}

	.tab-icon {
		font-size: 0.95rem;
	}

	.header-actions {
		display: flex;
		align-items: center;
		gap: 0.65rem;
		flex-wrap: wrap;
	}

	.btn {
		display: inline-flex;
		align-items: center;
		gap: 0.45rem;
		padding: 0.45rem 0.85rem;
		font-size: 0.825rem;
		font-weight: 500;
		border-radius: 0.45rem;
		text-decoration: none;
		transition: all 0.15s ease;
		color: var(--color-fg);
		cursor: pointer;
	}

	.btn-outline {
		border: 1px solid var(--color-line);
		background: var(--color-surface);
	}

	.btn-outline:hover {
		border-color: var(--color-outline);
		background: var(--color-surface-high);
		color: var(--color-heading);
	}

	.key-btn.configured {
		border-color: var(--color-status-success-border);
		color: var(--color-status-success);
	}

	.forgejo .dot {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
		background: var(--color-status-success);
	}

	@media (max-width: 900px) {
		.tagline {
			display: none;
		}
	}
</style>
