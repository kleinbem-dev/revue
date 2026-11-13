<script lang="ts">
	import '../app.css';
	import favicon from '#lib/assets/favicon.svg';
	import Header from '#lib/components/Header.svelte';
	import ReviewDetail from '#lib/components/ReviewDetail.svelte';
	import TriggerModal from '#lib/components/TriggerModal.svelte';
	import { apiFetch, getStoredApiKey, setStoredApiKey } from '#lib';
	import { page } from '$app/state';
	import { onNavigate, invalidate, goto } from '$app/navigation';
	import type { Snippet } from 'svelte';
	import type { LayoutData } from './$types';

	let { children, data }: { children?: Snippet; data: LayoutData } = $props();

	let apiKeyInput = $state(getStoredApiKey());
	let showKeyEditor = $state(false);

	onNavigate((navigation) => {
		if (!document.startViewTransition) return;
		return new Promise((resolve) => {
			document.startViewTransition(async () => {
				resolve();
				await navigation.complete;
			});
		});
	});

	const selectedReview = $derived(
		page.state.selectedReviewId
			? data.reviews.find((r) => r.id === page.state.selectedReviewId)
			: page.url.searchParams.get('review')
				? data.reviews.find((r) => r.id === page.url.searchParams.get('review'))
				: undefined
	);

	function closeReview() {
		if (page.state.selectedReviewId) {
			history.back();
		} else {
			goto(page.url.pathname, { replaceState: true });
		}
	}

	async function saveApiKey(e: SubmitEvent) {
		e.preventDefault();
		setStoredApiKey(apiKeyInput);
		showKeyEditor = false;
		await Promise.all([invalidate('app:status'), invalidate('app:reviews')]);
	}

	async function clearApiKey() {
		apiKeyInput = '';
		setStoredApiKey('');
		showKeyEditor = false;
		await Promise.all([invalidate('app:status'), invalidate('app:reviews')]);
	}

	async function handleTriggerReview(repo: string, pr: number, publish: boolean = false) {
		const res = await apiFetch('/api/trigger', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ repo, pr, publish })
		});
		if (!res.ok) {
			let errMsg = `HTTP ${res.status}`;
			const raw = await res.text();
			if (raw) {
				try {
					const body = JSON.parse(raw);
					errMsg = body.error || body.message || raw;
				} catch {
					errMsg = raw;
				}
			}
			throw new Error(errMsg);
		}
		await Promise.all([invalidate('app:status'), invalidate('app:reviews')]);
	}
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<div class="layout">
	<Header
		version={data.version}
		forgeUrl={data.forgeUrl}
		hasApiKey={Boolean(apiKeyInput.trim())}
		onToggleApiKey={() => (showKeyEditor = !showKeyEditor)}
	/>

	<main class="content">
		{#if data.authRequired || showKeyEditor}
			<form class="auth-banner" onsubmit={saveApiKey}>
				<div class="auth-copy">
					<strong>
						{data.authRequired
							? 'API Authentication Required'
							: 'Configure Revue API Key'}
					</strong>
					<span>
						Enter your <code>REVUE_API_KEY</code> to access protected review history, feedback constraints, and on-demand triggers.
					</span>
				</div>
				<div class="auth-controls">
					<input
						type="password"
						bind:value={apiKeyInput}
						placeholder="X-Revue-Key..."
						aria-label="Revue API Key"
						autocomplete="off"
					/>
					<button type="submit" class="auth-save">Save Key</button>
					{#if apiKeyInput.trim()}
						<button type="button" class="auth-clear" onclick={clearApiKey}>Clear</button>
					{/if}
				</div>
			</form>
		{/if}

		{#if children}
			{@render children()}
		{/if}
	</main>

	{#if selectedReview}
		<ReviewDetail review={selectedReview} onClose={closeReview} />
	{/if}

	{#if page.state.isTriggerOpen}
		<TriggerModal
			isOpen={true}
			providers={data.providers}
			recentReviews={data.reviews}
			onClose={() => history.back()}
			onTrigger={handleTriggerReview}
		/>
	{/if}
</div>

<style>
	:global(body) {
		margin: 0;
		background-color: var(--color-bg);
		color: var(--color-fg);
	}

	.layout {
		min-height: 100vh;
		display: flex;
		flex-direction: column;
	}

	.content {
		max-width: 1440px;
		width: 100%;
		margin: 0 auto;
		padding: 2rem;
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		gap: 2.25rem;
	}

	.auth-banner {
		display: flex;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 1rem;
		padding: 1rem 1.25rem;
		border-radius: 0.75rem;
		background: var(--color-status-warning-soft);
		border: 1px solid var(--color-status-warning-border);
	}

	.auth-copy {
		display: flex;
		flex-direction: column;
		gap: 0.2rem;
		font-size: 0.85rem;
		color: var(--color-fg);
	}

	.auth-copy strong {
		color: var(--color-heading);
		font-size: 0.925rem;
	}

	.auth-controls {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.auth-controls input {
		background: var(--color-bg);
		border: 1px solid var(--color-outline);
		border-radius: 0.45rem;
		padding: 0.45rem 0.75rem;
		font-size: 0.85rem;
		color: var(--color-heading);
		min-width: 220px;
	}

	.auth-save,
	.auth-clear {
		padding: 0.45rem 0.9rem;
		border-radius: 0.45rem;
		font-size: 0.825rem;
		font-weight: 600;
		cursor: pointer;
	}

	.auth-save {
		background: var(--color-accent);
		color: var(--color-accent-fg);
		border: 1px solid transparent;
	}

	.auth-save:hover {
		background: var(--color-accent-hover);
	}

	.auth-clear {
		background: var(--color-surface);
		color: var(--color-fg);
		border: 1px solid var(--color-line);
	}
</style>
