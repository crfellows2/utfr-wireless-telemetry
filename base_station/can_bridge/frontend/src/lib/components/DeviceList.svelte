<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { connectMqtt, subscribeTopic, unsubscribeTopic } from '$lib/mqtt';

	interface DeviceState {
		devices: string[];
		connected: string | null;
	}

	interface StorageInfo {
		used_kb: number;
		total_kb: number;
		used_mb: number;
		total_mb: number;
	}

	let state: DeviceState = $state({ devices: [], connected: null });
	let pendingDeviceId: string | null = $state(null);
	let storage: StorageInfo | null = $state(null);
	let es: EventSource;

	async function toggleConnection(deviceId: string) {
		const isConnected = state.connected === deviceId;
		pendingDeviceId = deviceId;

		try {
			const endpoint = isConnected ? 'disconnect' : 'connect';
			const url = isConnected
				? '/api/devices/disconnect'
				: `/api/devices/${encodeURIComponent(deviceId)}/connect`;

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

	function getStoragePercentage(): number {
		if (!storage || storage.total_kb === 0) return 0;
		return Math.round((storage.used_kb / storage.total_kb) * 100);
	}

	function getStorageColor(): string {
		const percent = getStoragePercentage();
		if (percent >= 90) return '#f44336'; // red
		if (percent >= 75) return '#ff9800'; // orange
		return '#4caf50'; // green
	}

	onMount(() => {
		// SSE for device list
		es = new EventSource('/api/devices/stream');
		es.onmessage = (e) => {
			state = JSON.parse(e.data);
			pendingDeviceId = null; // Clear loading state when SSE confirms
		};
		es.onerror = (err) => {
			console.error('EventSource error:', err);
		};

		// MQTT for storage data
		const brokerUrl = `ws://${window.location.hostname}:9001`;
		connectMqtt(brokerUrl);

		subscribeTopic('link/storage', (data) => {
			storage = data;
		});

		return () => {
			unsubscribeTopic('link/storage');
		};
	});

	onDestroy(() => {
		es?.close();
		unsubscribeTopic('link/storage');
	});
</script>

<div class="device-list-container">
	<h2>USB Devices</h2>

	{#if state.devices.length === 0}
		<p class="no-devices">No devices connected</p>
	{:else}
		<div class="device-list">
			{#each state.devices as deviceId}
				<div class="device-card" class:active={state.connected === deviceId}>
					<div class="device-id">{deviceId}</div>
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

	{#if storage}
		<div class="storage-section">
			<h3>SD Card</h3>
			<div class="progress-bar">
				<div
					class="progress-fill"
					style="width: {getStoragePercentage()}%; background-color: {getStorageColor()}"
				></div>
			</div>
			<div class="storage-info">
				<span>
					{#if storage.used_mb < 1024}
						{storage.used_mb} MB
					{:else}
						{(storage.used_mb / 1024).toFixed(1)} GB
					{/if}
					/ {(storage.total_mb / 1024).toFixed(1)} GB
				</span>
				<span>{getStoragePercentage()}% used</span>
			</div>
		</div>
	{/if}
</div>

<style>
	.device-list-container {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	h2 {
		font-size: 1.25rem;
		margin: 0;
	}

	h3 {
		font-size: 1rem;
		margin: 0;
		font-weight: 600;
	}

	.no-devices {
		color: #666;
		font-style: italic;
		font-size: 0.9rem;
	}

	.device-list {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.device-card {
		border: 1px solid #ddd;
		border-radius: 6px;
		padding: 0.75rem;
		background: #f9f9f9;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		transition: all 0.2s ease;
	}

	.device-card.active {
		border-color: #4caf50;
		background: #e8f5e9;
	}

	.device-id {
		font-family: monospace;
		font-size: 0.85rem;
		word-break: break-all;
	}

	button {
		padding: 0.4rem 0.75rem;
		border: 1px solid #ccc;
		border-radius: 4px;
		background: white;
		cursor: pointer;
		font-size: 0.85rem;
		transition: all 0.2s ease;
		width: 100%;
	}

	button:hover:not(:disabled) {
		background: #f0f0f0;
		border-color: #999;
	}

	button:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.device-card.active button {
		background: #4caf50;
		color: white;
		border-color: #4caf50;
	}

	.device-card.active button:hover:not(:disabled) {
		background: #45a049;
	}

	.storage-section {
		margin-top: 1.5rem;
		padding-top: 1.5rem;
		border-top: 1px solid #e0e0e0;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.progress-bar {
		width: 100%;
		height: 20px;
		background: #e0e0e0;
		border-radius: 10px;
		overflow: hidden;
	}

	.progress-fill {
		height: 100%;
		transition: width 0.3s ease, background-color 0.3s ease;
		border-radius: 10px;
	}

	.storage-info {
		display: flex;
		justify-content: space-between;
		font-size: 0.85rem;
		color: #666;
	}

	.write-speed {
		font-size: 0.85rem;
		color: #666;
		font-family: monospace;
	}
</style>
