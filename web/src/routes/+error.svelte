<script lang="ts">
	import { page } from '$app/state';
</script>

<div class="error-container">
	<h1>{page.status}</h1>
	<p>{page.error?.message ?? 'An unexpected error occurred'}</p>
	<a href="/">Back to Dashboard</a>
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
	}
	h1 {
		font-size: 3rem;
		color: #ef4444;
		margin: 0;
	}
	p {
		color: #94a3b8;
		font-size: 1.15rem;
	}
	a {
		color: #38bdf8;
		text-decoration: none;
		padding: 0.5rem 1rem;
		border: 1px solid #38bdf8;
		border-radius: 0.5rem;
	}
</style>
