<script lang="ts">
	// Under the board in Generation mode: the score of every layout bbgen tried, and every step in a log. Clicking
	// either shows the board as it was at that step.
	import { tick } from 'svelte';
	import { studio } from '#lib/studio.svelte.js';
	import ScoreChart from './ScoreChart.svelte';
	import Check from '@lucide/svelte/icons/check';
	import X from '@lucide/svelte/icons/x';

	const steps = $derived(studio.ofFile);
	const shown = $derived(studio.shownStep);
	const fmt = (s: number | null) => (s === null ? '–' : String(s));

	// the log follows the newest step, or keeps the shown one in view
	let log = $state<HTMLDivElement>();
	$effect(() => {
		const live = studio.live;
		const n = live ? steps.length : shown?.n;
		if (!log || n === undefined) return;
		tick().then(() => {
			if (live) log!.scrollTop = log!.scrollHeight;
			else log!.querySelector(`[data-n="${n}"]`)?.scrollIntoView({ block: 'nearest' });
		});
	});
</script>

<div class="flex h-full bg-background">
	<ScoreChart class="flex-[3]" {steps} shown={shown?.n ?? null} onpick={(n) => studio.showStep(n)} />
	<div bind:this={log} class="min-w-0 flex-[2] overflow-y-auto border-l py-1 font-mono text-[11px]" aria-label="Steps">
		{#each steps as e (e.n)}
			<button
				type="button"
				data-n={e.n}
				disabled={!e.svg}
				onclick={() => studio.showStep(e.n)}
				class="flex w-full items-center gap-2 px-2 py-0.5 text-left {e.n === shown?.n
					? 'bg-primary/15'
					: e.svg
						? 'hover:bg-muted'
						: ''} {e.kept ? '' : 'text-muted-foreground'}"
			>
				<span class="w-8 shrink-0 text-right text-muted-foreground">{e.n}</span>
				{#if e.phase === 'wiring'}<span class="size-3 shrink-0"></span>{:else if e.kept}<Check class="size-3 shrink-0 text-primary" />{:else}<X class="size-3 shrink-0 opacity-50" />{/if}
				<span class="min-w-0 flex-1 truncate">{e.caption}</span>
				{#if e.score !== null}<span class="shrink-0">{fmt(e.from)} → {e.score}</span>{/if}
			</button>
		{:else}
			<div class="px-3 py-2 font-sans text-xs text-muted-foreground">Each step bbgen takes shows here as it takes it.</div>
		{/each}
	</div>
</div>
