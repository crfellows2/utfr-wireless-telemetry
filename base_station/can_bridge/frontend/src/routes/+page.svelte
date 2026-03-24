<script lang="ts">
	import DeviceList from '$lib/components/DeviceList.svelte';
	import ConfigEditor from '$lib/components/ConfigEditor.svelte';
</script>

<div class="container">
	<aside class="sidebar">
		<DeviceList />
	</aside>
	<main class="main-content">
		<ConfigEditor />
	</main>
</div>

<style>
	.container {
		display: grid;
		grid-template-columns: 280px 1fr;
		gap: 2rem;
		padding: 2rem;
		min-height: 100vh;
		max-width: 1600px;
		margin: 0 auto;
	}

	.sidebar {
		border-right: 1px solid #e0e0e0;
		padding-right: 2rem;
	}

	.main-content {
		overflow-y: auto;
		min-width: 0; /* Prevents grid blowout */
	}

	/* Responsive layout for smaller screens */
	@media (max-width: 768px) {
		.container {
			grid-template-columns: 1fr;
			gap: 1.5rem;
		}

		.sidebar {
			border-right: none;
			border-bottom: 1px solid #e0e0e0;
			padding-right: 0;
			padding-bottom: 1.5rem;
		}
	}
</style>
