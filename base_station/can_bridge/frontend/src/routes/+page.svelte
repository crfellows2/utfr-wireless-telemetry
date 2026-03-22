<script lang="ts">
	import { onMount, onDestroy } from 'svelte';

	interface DeviceState {
		devices: string[];
		connected: string | null;
	}

	let state: DeviceState = $state({ devices: [], connected: null });
	let pendingDeviceId: string | null = $state(null);
	let es: EventSource;

	async function toggleConnection(deviceId: string) {
		const isConnected = state.connected === deviceId;
		pendingDeviceId = deviceId;

		try {
			const endpoint = isConnected ? 'disconnect' : 'connect';
			const url = isConnected ? '/api/devices/disconnect' : `/api/devices/${deviceId}/connect`;

			const response = await fetch(url, { method: 'POST' });

			if (!response.ok) {
				throw new Error(`Failed to ${endpoint}: ${response.statusText}`);
			}

			// Wait for SSE to confirm - pendingDeviceId cleared in onmessage
		} catch (err) {
			console.error('Connection failed:', err);
			pendingDeviceId = null; // Clear on error
		}
	}

	onMount(() => {
		es = new EventSource('/api/devices/stream');
		es.onmessage = (e) => {
			state = JSON.parse(e.data);
			pendingDeviceId = null; // Clear loading state when SSE confirms
		};
		es.onerror = (err) => {
			console.error('EventSource error:', err);
		};
	});

	onDestroy(() => {
		es?.close();
	});
</script>

<div class="container">
	<h1>USB Devices</h1>

	{#if state.devices.length === 0}
		<p class="no-devices">No devices connected</p>
	{:else}
		<div class="device-list">
			{#each state.devices as deviceId}
				<div class="device-card" class:active={state.connected === deviceId}>
					<span class="device-id">{deviceId}</span>
					<button
						onclick={() => toggleConnection(deviceId)}
						disabled={pendingDeviceId !== null}
						class:loading={pendingDeviceId === deviceId}
					>
						{#if pendingDeviceId === deviceId}
							Loading...
						{:else if state.connected === deviceId}
							Disconnect
						{:else}
							Connect
						{/if}
					</button>
				</div>
			{/each}
		</div>
	{/if}
</div>

<style>
	.container {
		max-width: 800px;
		margin: 2rem auto;
		padding: 1rem;
	}

	h1 {
		margin-bottom: 1.5rem;
	}

	.no-devices {
		color: #666;
		font-style: italic;
	}

	.device-list {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	.device-card {
		border: 1px solid #ddd;
		border-radius: 8px;
		padding: 1rem;
		background: #f9f9f9;
		display: flex;
		justify-content: space-between;
		align-items: center;
		transition: all 0.2s ease;
	}

	.device-card.active {
		border-color: #4caf50;
		background: #e8f5e9;
	}

	.device-id {
		font-family: monospace;
		font-size: 0.95rem;
	}

	button {
		padding: 0.5rem 1rem;
		border: 1px solid #ccc;
		border-radius: 4px;
		background: white;
		cursor: pointer;
		font-size: 0.9rem;
		transition: all 0.2s ease;
	}

	button:hover:not(:disabled) {
		background: #f0f0f0;
		border-color: #999;
	}

	button:disabled {
		opacity: 0.6;
	}

	.device-card.active button {
		background: #4caf50;
		color: white;
		border-color: #4caf50;
	}

	.device-card.active button:hover:not(:disabled) {
		background: #45a049;
	}
</style>
