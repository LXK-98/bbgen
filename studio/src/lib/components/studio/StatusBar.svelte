<script lang="ts">
	// What's under the pointer, down to the hole and its strip.
	import { studio } from '#lib/studio.svelte.js';
	import { describe } from '#lib/describe.js';
	import type { Key } from '#lib/scene.js';

	const scene = $derived(studio.scene);
	const hole = $derived(scene && studio.index && studio.hole ? describe(`hole:${studio.hole}` as Key, scene, studio.index) : null);
	const thing = $derived(
		scene && studio.index && studio.hover && !studio.hover.startsWith('hole:') ? describe(studio.hover, scene, studio.index) : null
	);
</script>

<footer class="flex h-7 shrink-0 items-center gap-4 overflow-hidden border-t bg-muted/40 px-3 text-xs whitespace-nowrap text-muted-foreground">
	{#if hole}
		<span class="font-mono text-foreground">{hole.title.replace('hole ', '')}</span>
		<span class="truncate font-mono">{hole.lines.join(' · ')}</span>
	{/if}
	{#if thing}
		<span class="truncate font-mono"><span class="text-foreground">{thing.title}</span> · {thing.lines[0]}</span>
	{/if}
	{#if !hole && !thing}
		<span>Scroll to zoom, drag to pan, F to fit, Esc to clear the selection</span>
	{/if}
	<span class="ml-auto">{#if scene}bbgen {scene.bbgen} · {studio.engine?.kind === 'tauri' ? 'desktop' : 'browser'}{/if}</span>
</footer>
