<script lang="ts">
	import type { ProviderStatus } from '../types';

	let { provider }: { provider: ProviderStatus } = $props();

	const isConfigured = $derived(
		provider.status === 'configured' ||
			provider.status === 'active' ||
			provider.status === 'ready'
	);
</script>

<div class="card" class:active={isConfigured} class:unconfigured={provider.status === 'unconfigured'}>
	<div class="card-header">
		<div class="title-wrap">
			<span class="status-indicator {provider.status}" title="Status: {provider.status}"></span>
			<h3>{provider.name}</h3>
		</div>
		<span class="type-badge" class:ready={isConfigured}>
			{provider.type}
		</span>
	</div>

	<div class="model-info">
		<span class="label">Model:</span>
		<code>{provider.model}</code>
	</div>

	<div class="card-footer">
		<div class="stat">
			<span class="stat-label">Cost:</span>
			<span class="stat-value" class:free={provider.cost_per_token.startsWith('$0')}>
				{provider.cost_per_token}
			</span>
		</div>
		<div class="stat">
			<span class="stat-label">Status:</span>
			<span class="stat-value">{provider.status}</span>
		</div>
	</div>
</div>

<style>
	.card {
		background: var(--color-surface);
		border: 1px solid var(--color-line);
		border-radius: 0.75rem;
		padding: 1.15rem;
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
		transition: border-color 0.15s ease, transform 0.15s ease;
	}

	.card:hover {
		border-color: var(--color-outline);
		transform: translateY(-1px);
	}

	.card.unconfigured {
		opacity: 0.72;
	}

	.card-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.5rem;
	}

	.title-wrap {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	h3 {
		font-size: 0.95rem;
		font-weight: 600;
		color: var(--color-heading);
		margin: 0;
	}

	.status-indicator {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
		background: var(--color-muted);
		flex-shrink: 0;
	}

	.status-indicator.configured,
	.status-indicator.active,
	.status-indicator.ready {
		background: var(--color-status-success);
	}

	.status-indicator.unconfigured {
		background: var(--color-status-warning);
	}

	.type-badge {
		font-size: 0.7rem;
		font-weight: 600;
		padding: 0.15rem 0.45rem;
		border-radius: 0.35rem;
		background: var(--color-surface-high);
		color: var(--color-muted);
		border: 1px solid var(--color-line);
	}

	.type-badge.ready {
		background: var(--color-accent-soft);
		color: var(--color-accent-soft-fg);
		border-color: transparent;
	}

	.model-info {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.8rem;
		color: var(--color-muted);
	}

	code {
		font-size: 0.775rem;
		background: var(--color-bg);
		padding: 0.15rem 0.4rem;
		border-radius: 0.25rem;
		border: 1px solid var(--color-line);
		color: var(--color-heading);
	}

	.card-footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding-top: 0.65rem;
		border-top: 1px solid var(--color-line);
		font-size: 0.775rem;
	}

	.stat {
		display: flex;
		gap: 0.35rem;
	}

	.stat-label {
		color: var(--color-muted);
	}

	.stat-value {
		font-weight: 500;
		color: var(--color-fg);
	}

	.stat-value.free {
		color: var(--color-status-success);
	}
</style>
