<script lang="ts">
	import favicon from '#lib/assets/favicon.svg';
	import Header from '#lib/components/Header.svelte';
	import ReviewDetail from '#lib/components/ReviewDetail.svelte';
	import TriggerModal from '#lib/components/TriggerModal.svelte';
	import { page } from '$app/state';
	import { onNavigate, invalidate, goto } from '$app/navigation';
	import type { Snippet } from 'svelte';
	import type { LayoutData } from './$types';

	let { children, data }: { children?: Snippet; data: LayoutData } = $props();

	// SvelteKit View Transitions API integration
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
			// Direct deep link arrival: clear review query param without leaving dashboard
			goto(page.url.pathname, { replaceState: true });
		}
	}

	async function handleTriggerReview(repo: string, pr: number, publish: boolean = false) {
		const res = await fetch('/api/trigger', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ repo, pr, number: pr, publish })
		});
		if (!res.ok) {
			const text = await res.text();
			throw new Error(text || 'Failed to queue review');
		}
		await Promise.all([
			invalidate('app:status'),
			invalidate('app:reviews')
		]);
	}
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<div class="layout">
	<Header version="0.1.0" />

	<main class="content">
		{#if children}
			{@render children()}
		{/if}
	</main>

	<!-- SvelteKit Shallow Routing Modals (Page State) -->
	{#if selectedReview}
		<ReviewDetail
			review={selectedReview}
			onClose={closeReview}
		/>
	{/if}

	{#if page.state.isTriggerOpen}
		<TriggerModal
			isOpen={true}
			onClose={() => history.back()}
			onTrigger={handleTriggerReview}
		/>
	{/if}
</div>

<style>
	:root {
		--bg-base: #0f131a;
		--bg-card: #161b22;
		--border: rgba(255, 255, 255, 0.08);
		--text: #f0f6fc;
		--text-muted: #8b949e;
	}

	:global(body) {
		margin: 0;
		background-color: var(--bg-base);
		color: var(--text);
		font-family:
			system-ui,
			-apple-system,
			BlinkMacSystemFont,
			'Segoe UI',
			Roboto,
			Oxygen,
			Ubuntu,
			Cantarell,
			sans-serif;
		-webkit-font-smoothing: antialiased;
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
</style>
