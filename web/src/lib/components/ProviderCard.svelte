<script lang="ts">
	import type { ProviderStatus } from '../types';

	let { provider }: { provider: ProviderStatus } = $props();

	const isLive = $derived(provider.status === 'active' || provider.status === 'ready');
</script>

<div class="card" class:active={provider.status === 'active'}>
	<div class="card-header">
		<div class="title-wrap">
			<span class="status-indicator {provider.status}"></span>
			<h3>{provider.name}</h3>
		</div>
		<span class="type-badge {provider.type}">
			{provider.type === 'subscription' ? '$0 Sub' : provider.type}
		</span>
	</div>

	<div class="model-info">
		<span class="label">Model:</span>
		<code>{provider.model}</code>
	</div>

	<div class="card-footer">
		<div class="stat">
			<span class="stat-label">Cost:</span>
			<span class="stat-value free">{provider.cost_per_token}</span>
		</div>
		<div class="stat">
			<span class="stat-label">Avg Latency:</span>
			<span class="stat-value">{provider.latency_avg}</span>
		</div>
	</div>
</div>

<style>
	.card {
		background: rgba(15, 23, 42, 0.6);
		border: 1px solid #1e293b;
		border-radius: 0.75rem;
		padding: 1.15rem;
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
		transition: all 0.2s ease;
		backdrop-filter: blur(8px);
	}

	.card:hover {
		border-color: #334155;
		transform: translateY(-1px);
	}

	.card.active {
		border-color: rgba(139, 92, 246, 0.4);
		box-shadow: 0 0 15px rgba(139, 92, 246, 0.1);
	}

	.card-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.title-wrap {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	h3 {
		font-size: 0.95rem;
		font-weight: 600;
		color: #f1f5f9;
		margin: 0;
	}

	.status-indicator {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
	}

	.status-indicator.active {
		background: #10b981;
		box-shadow: 0 0 8px #10b981;
	}

	.status-indicator.ready {
		background: #3b82f6;
	}

	.status-indicator.fallback {
		background: #f59e0b;
	}

	.type-badge {
		font-size: 0.7rem;
		font-weight: 600;
		padding: 0.15rem 0.45rem;
		border-radius: 0.35rem;
		background: #1e293b;
		color: #94a3b8;
		border: 1px solid #334155;
	}

	.type-badge.subscription {
		background: rgba(16, 185, 129, 0.1);
		color: #34d399;
		border-color: rgba(16, 185, 129, 0.3);
	}

	.model-info {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.8rem;
		color: #94a3b8;
	}

	code {
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.775rem;
		background: #0f172a;
		padding: 0.15rem 0.4rem;
		border-radius: 0.25rem;
		border: 1px solid #1e293b;
		color: #cbd5e1;
	}

	.card-footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding-top: 0.65rem;
		border-top: 1px solid #1e293b;
		font-size: 0.775rem;
	}

	.stat {
		display: flex;
		gap: 0.35rem;
	}

	.stat-label {
		color: #64748b;
	}

	.stat-value {
		font-weight: 500;
		color: #e2e8f0;
	}

	.stat-value.free {
		color: #34d399;
	}
</style>
