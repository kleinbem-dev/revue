<script lang="ts">
	import { page } from '$app/state';

	function hardReset() {
		try {
			localStorage.clear();
			sessionStorage.clear();
		} catch {}
		window.location.href = '/' + '?t=' + Date.now();
	}
</script>

<div class="error-container">
	<h1>{page.status}</h1>
	<p class="error-msg">{page.error?.message ?? 'An unexpected error occurred'}</p>
	{#if page.error?.stack}
		<pre class="error-stack">{page.error.stack}</pre>
	{/if}
	<div class="actions">
		<a href="/">Back to Dashboard</a>
		<button class="reset-btn" onclick={hardReset}>Hard Reset & Clear Cache</button>
	</div>
</div>

<style>
	.error-container {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		min-height: 80vh;
		color: #f8fafc;
		font-family: sans-serif;
		gap: 1rem;
		padding: 2rem;
		max-width: 800px;
		margin: 0 auto;
	}
	h1 {
		font-size: 3rem;
		color: #ef4444;
		margin: 0;
	}
	.error-msg {
		color: #94a3b8;
		font-size: 1.15rem;
		text-align: center;
	}
	.error-stack {
		background: #0f172a;
		border: 1px solid #1e293b;
		padding: 1rem;
		border-radius: 0.5rem;
		color: #fca5a5;
		font-size: 0.85rem;
		overflow-x: auto;
		max-width: 100%;
		white-space: pre-wrap;
	}
	.actions {
		display: flex;
		gap: 1rem;
		margin-top: 1rem;
	}
	a {
		color: #38bdf8;
		text-decoration: none;
		padding: 0.5rem 1rem;
		border: 1px solid #38bdf8;
		border-radius: 0.5rem;
	}
	.reset-btn {
		background: #ef4444;
		color: #ffffff;
		border: none;
		padding: 0.5rem 1rem;
		border-radius: 0.5rem;
		cursor: pointer;
		font-weight: 600;
	}
	.reset-btn:hover {
		background: #dc2626;
	}
</style>
