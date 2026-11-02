<script lang="ts">
	let { version = '0.1.0' }: { version?: string } = $props();
</script>

<header class="header">
	<div class="logo-area">
		<span class="icon">🎭</span>
		<div class="brand">
			<h1>Revue</h1>
			<span class="badge">v{version}</span>
		</div>
		<span class="tagline">Self-hosted, forge-agnostic AI code review</span>
	</div>

	<div class="header-actions">
		<a
			href="https://github.com/kleinbem-dev/revue"
			target="_blank"
			rel="noreferrer"
			class="btn btn-outline"
		>
			<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
				<path
					d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"
				/>
			</svg>
			GitHub
		</a>
		<a
			href="https://git.kleinbem.dev"
			target="_blank"
			rel="noreferrer"
			class="btn btn-outline forgejo"
		>
			<span class="dot"></span>
			Forgejo
		</a>
	</div>
</header>

<style>
	.header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1.25rem 2rem;
		border-bottom: 1px solid var(--border-color, #1e293b);
		background: rgba(15, 23, 42, 0.75);
		backdrop-filter: blur(12px);
		position: sticky;
		top: 0;
		z-index: 50;
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
		background: linear-gradient(135deg, #f8fafc 0%, #cbd5e1 100%);
		-webkit-background-clip: text;
		background-clip: text;
		-webkit-text-fill-color: transparent;
		margin: 0;
	}

	.badge {
		font-size: 0.7rem;
		font-weight: 600;
		color: #94a3b8;
		background: #1e293b;
		padding: 0.15rem 0.5rem;
		border-radius: 9999px;
		border: 1px solid #334155;
	}

	.tagline {
		color: #64748b;
		font-size: 0.875rem;
		padding-left: 0.75rem;
		border-left: 1px solid #334155;
	}

	.header-actions {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.btn {
		display: inline-flex;
		align-items: center;
		gap: 0.45rem;
		padding: 0.45rem 0.95rem;
		font-size: 0.825rem;
		font-weight: 500;
		border-radius: 0.45rem;
		text-decoration: none;
		transition: all 0.15s ease;
		color: #cbd5e1;
	}

	.btn-outline {
		border: 1px solid #334155;
		background: rgba(30, 41, 59, 0.6);
	}

	.btn-outline:hover {
		border-color: #64748b;
		background: #1e293b;
		color: #f8fafc;
	}

	.forgejo .dot {
		width: 8px;
		height: 8px;
		border-radius: 9999px;
		background: #10b981;
		box-shadow: 0 0 6px #10b981;
	}

	@media (max-width: 640px) {
		.tagline {
			display: none;
		}
	}
</style>
