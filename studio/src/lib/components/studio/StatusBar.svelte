<script lang="ts">
	// What's under the pointer, down to the hole and its strip; while a build shows its steps, the step bbgen is on.
	import { studio } from '#lib/studio.svelte.js';
	import { describe } from '#lib/describe.js';
	import type { Key } from '#lib/scene.js';

	const scene = $derived(studio.scene);
	const hole = $derived(scene && studio.index && studio.hole ? describe(`hole:${studio.hole}` as Key, scene, studio.index) : null);
	const thing = $derived(
		scene && studio.index && studio.hover && !studio.hover.startsWith('hole:') ? describe(studio.hover, scene, studio.index) : null
	);
	const PHASES = { start: 'starting layouts', turn: 'turning parts', repair: 'moving parts', wiring: 'wiring' };
	const step = $derived(studio.stepping ? studio.step : null);
	/** the best layout so far: the latest kept placement step's score */
	const best = $derived(studio.stepping ? studio.steps.findLast((e) => e.kept && e.score !== null)?.score : undefined);
</script>

<footer class="flex h-7 shrink-0 items-center gap-4 overflow-hidden border-t bg-muted/40 px-3 text-xs whitespace-nowrap text-muted-foreground">
	{#if studio.stepping}
		<span class="flex items-center gap-1.5 text-foreground"><span class="size-1.5 animate-pulse rounded-full bg-primary"></span>
			{step ? `Step ${step.n} · ${PHASES[step.phase]}` : 'Starting…'}</span>
		{#if step}
			<span class="truncate font-mono">{step.caption}{#if step.score !== null}: {step.from ?? '–'} → {step.score} {step.kept ? '✓' : '✗'}{/if}</span>
			{#if best !== undefined}<span class="font-mono">best {best}</span>{/if}
		{/if}
	{:else if hole}
		<span class="font-mono text-foreground">{hole.title.replace('hole ', '')}</span>
		<span class="truncate font-mono">{hole.lines.join(' · ')}</span>
	{/if}
	{#if !studio.stepping && thing}
		<span class="truncate font-mono"><span class="text-foreground">{thing.title}</span> · {thing.lines[0]}</span>
	{/if}
	{#if !studio.stepping && !hole && !thing}
		<span>Scroll to zoom, drag to pan, F to fit, Esc to clear the selection</span>
	{/if}
	<span class="ml-auto">{#if scene}bbgen {scene.bbgen} · {studio.engine?.kind === 'tauri' ? 'desktop' : 'browser'}{/if}</span>
</footer>
