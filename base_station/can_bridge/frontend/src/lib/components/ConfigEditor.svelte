<script lang="ts">
	import { onMount } from 'svelte';

	let configText = $state('');
	let status = $state<'idle' | 'loading' | 'saving'>('loading');
	let message = $state<{ type: 'ok' | 'err'; text: string } | null>(null);
	let fontSize = $state(14);
	let selectedProfile = $state('');
	let profiles = $state<string[]>([]);

	function increaseFontSize() {
		fontSize = Math.min(24, fontSize + 2);
	}

	function decreaseFontSize() {
		fontSize = Math.max(10, fontSize - 2);
	}

	async function loadProfiles() {
		try {
			const response = await fetch('/api/profile');
			if (response.ok) {
				const data = await response.json();
				profiles = data.available;
				selectedProfile = data.active;
			}
		} catch (err) {
			console.error('Failed to load profiles:', err);
		}
	}

	async function changeProfile() {
		try {
			const response = await fetch('/api/profile', {
				method: 'PUT',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ profile: selectedProfile })
			});
			if (!response.ok) {
				const errorText = await response.text();
				console.error('Failed to change profile:', errorText);
			}
		} catch (err) {
			console.error('Failed to change profile:', err);
		}
	}

	async function loadConfig() {
		status = 'loading';
		message = null;

		try {
			const response = await fetch('/api/config');

			if (!response.ok) {
				throw new Error(`Failed to load config: ${response.statusText}`);
			}

			configText = await response.text();
			status = 'idle';
		} catch (err) {
			status = 'idle';
			message = {
				type: 'err',
				text: err instanceof Error ? err.message : 'Failed to load config'
			};
		}
	}

	async function saveConfig() {
		status = 'saving';
		message = null;

		try {
			const response = await fetch('/api/config', {
				method: 'POST',
				body: configText
			});

			if (response.ok) {
				message = { type: 'ok', text: 'Configuration saved successfully' };
			} else {
				const errorText = await response.text();
				message = { type: 'err', text: errorText || 'Failed to save config' };
			}

			status = 'idle';
		} catch (err) {
			status = 'idle';
			message = {
				type: 'err',
				text: err instanceof Error ? err.message : 'Failed to save config'
			};
		}
	}

	onMount(() => {
		loadProfiles();
		loadConfig();
	});
</script>

<div class="config-editor">
	<h2>Filter Profile Configuration</h2>

	{#if profiles.length > 0}
		<div class="profile-selector">
			<label for="profile">Active Profile:</label>
			<select id="profile" bind:value={selectedProfile} onchange={changeProfile}>
				{#each profiles as profile}
					<option value={profile}>{profile}</option>
				{/each}
			</select>
		</div>
	{/if}

	{#if status === 'loading'}
		<div class="loading">
			<p>Loading configuration...</p>
		</div>
	{:else}
		<div class="font-controls">
			<button
				class="font-btn"
				onclick={decreaseFontSize}
				disabled={fontSize <= 10}
				title="Decrease font size"
				aria-label="Decrease font size"
			>
				A-
			</button>
			<button
				class="font-btn"
				onclick={increaseFontSize}
				disabled={fontSize >= 24}
				title="Increase font size"
				aria-label="Increase font size"
			>
				A+
			</button>
		</div>
		<textarea
			bind:value={configText}
			disabled={status === 'saving'}
			placeholder="Configuration will appear here..."
			spellcheck="false"
			style="font-size: {fontSize}px;"
		></textarea>

		<div class="actions">
			<button onclick={saveConfig} disabled={status === 'saving'}>
				{status === 'saving' ? 'Saving...' : 'Save Configuration'}
			</button>

			{#if message}
				<div class="message {message.type}">
					{message.text}
				</div>
			{/if}
		</div>
	{/if}
</div>

<style>
	.config-editor {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		height: 100%;
	}

	h2 {
		font-size: 1.5rem;
		margin: 0;
	}

	.font-controls {
		display: flex;
		gap: 0.5rem;
		justify-content: flex-end;
		margin-bottom: 0.5rem;
	}

	.font-btn {
		padding: 0.3rem 0.6rem;
		border: 1px solid #ccc;
		border-radius: 4px;
		background: white;
		color: #666;
		cursor: pointer;
		font-size: 0.9rem;
		font-weight: 500;
		transition: all 0.2s ease;
		min-width: 36px;
	}

	.font-btn:hover:not(:disabled) {
		background: #f0f0f0;
		border-color: #999;
	}

	.font-btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}

	.profile-selector {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}

	.profile-selector label {
		font-size: 0.95rem;
		font-weight: 500;
		color: #333;
	}

	.profile-selector select {
		padding: 0.4rem 0.75rem;
		border: 1px solid #ddd;
		border-radius: 4px;
		background: #fafafa;
		font-size: 0.95rem;
		cursor: pointer;
		transition: all 0.2s ease;
	}

	.profile-selector select:hover {
		border-color: #999;
		background: white;
	}

	.profile-selector select:focus {
		outline: none;
		border-color: #4caf50;
		background: white;
	}

	.loading {
		color: #666;
		font-style: italic;
	}

	textarea {
		width: 100%;
		height: 60vh;
		min-height: 400px;
		font-family: 'Courier New', Consolas, monospace;
		font-size: 14px;
		line-height: 1.5;
		padding: 1rem;
		border: 1px solid #ddd;
		border-radius: 6px;
		resize: vertical;
		background: #fafafa;
		transition: all 0.2s ease;
	}

	textarea:focus {
		outline: none;
		border-color: #4caf50;
		background: white;
	}

	textarea:disabled {
		opacity: 0.6;
		cursor: not-allowed;
		background: #f5f5f5;
	}

	.actions {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	button {
		padding: 0.75rem 1.5rem;
		border: 1px solid #4caf50;
		border-radius: 6px;
		background: #4caf50;
		color: white;
		cursor: pointer;
		font-size: 1rem;
		font-weight: 500;
		transition: all 0.2s ease;
		align-self: flex-start;
	}

	button:hover:not(:disabled) {
		background: #45a049;
		border-color: #45a049;
	}

	button:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.message {
		padding: 0.75rem 1rem;
		border-radius: 6px;
		font-size: 0.95rem;
		white-space: pre-wrap;
		font-family: monospace;
		animation: slideIn 0.2s ease;
	}

	.message.ok {
		background: #e8f5e9;
		color: #2e7d32;
		border: 1px solid #a5d6a7;
	}

	.message.err {
		background: #ffebee;
		color: #c62828;
		border: 1px solid #ef9a9a;
	}

	@keyframes slideIn {
		from {
			opacity: 0;
			transform: translateY(-10px);
		}
		to {
			opacity: 1;
			transform: translateY(0);
		}
	}
</style>
