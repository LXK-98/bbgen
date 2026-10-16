<script lang="ts">
	// Over the centre while files are dragged over the window: what a drop would open, or why it won't.
	import { studio } from '#lib/studio.svelte.js';
	import FileDown from '@lucide/svelte/icons/file-down';
	import FileX from '@lucide/svelte/icons/file-x';

	const drag = $derived(studio.dragging);
	const name = (p: string) => p.split(/[\\/]/).pop();
</script>

{#if drag}
	<div
		class={[
			'pointer-events-none absolute inset-2 z-20 flex flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed bg-background/85',
			drag.circuit || !drag.paths ? 'border-primary' : 'border-destructive'
		]}
	>
		{#if drag.circuit}
			<FileDown class="size-8 text-primary" />
			<p class="text-sm font-medium">Drop to open <span class="font-mono">{name(drag.circuit)}</span></p>
		{:else if drag.paths}
			<FileX class="size-8 text-destructive" />
			<p class="text-sm font-medium">Not a circuit file: drop a .yaml or .yml</p>
		{:else}
			<FileDown class="size-8 text-primary" />
			<p class="text-sm font-medium">Drop a circuit file</p>
			<p class="text-xs text-muted-foreground">A browser doesn't say where it is, so you'll be asked for its path</p>
		{/if}
	</div>
{/if}
