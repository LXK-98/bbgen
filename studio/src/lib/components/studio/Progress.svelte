<script lang="ts">
	// Under the board in Generation mode: the score of every layout bbgen tried, and every step in a log. Clicking
	// either shows the board as it was at that step. Right-clicking a kept step in the log picks it to compare,
	// and then another compares the two.
	import { tick } from 'svelte';
	import { studio } from '#lib/studio.svelte.js';
	import ScoreChart from './ScoreChart.svelte';
	import Check from '@lucide/svelte/icons/check';
	import X from '@lucide/svelte/icons/x';
	import * as ContextMenu from '#lib/components/ui/context-menu/index.js';
	import GitCompareArrows from '@lucide/svelte/icons/git-compare-arrows';
	import Pin from '@lucide/svelte/icons/pin';

	const steps = $derived(studio.ofFile);
	const shown = $derived(studio.shownStep);
	const fmt = (s: number | null) => (s === null ? '–' : String(s));
	const lit = (n: number) => n === shown?.n || !!studio.compare?.includes(n);

	/** the kept step the menu is for: the row right-clicked */
	let menuFor = $state<number | null>(null);
	function oncontextmenu(e: MouseEvent) {
		const row = (e.target as Element).closest<HTMLElement>('[data-n]');
		const n = Number(row?.dataset.n);
		if (!row || !steps.find((x) => x.n === n)?.svg) {
			// only a kept step has a board to compare; elsewhere, no menu
			e.preventDefault();
			e.stopPropagation();
			return;
		}
		menuFor = n;
	}

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
	<ContextMenu.Root>
		<ContextMenu.Trigger>
			{#snippet child({ props })}
				<div {...props} bind:this={log} oncontextmenucapture={oncontextmenu}
					class="min-w-0 flex-[2] overflow-y-auto border-l py-1 font-mono text-[11px]" aria-label="Steps">
					{#each steps as e (e.n)}
						<button
							type="button"
							data-n={e.n}
							aria-disabled={!e.svg}
							onclick={() => e.svg && studio.showStep(e.n)}
							class="flex w-full items-center gap-2 px-2 py-0.5 text-left {lit(e.n)
								? 'bg-primary/15'
								: e.svg
									? 'hover:bg-muted'
									: 'cursor-default'} {e.kept ? '' : 'text-muted-foreground'}"
						>
							<span class="w-8 shrink-0 text-right text-muted-foreground">{e.n}</span>
							{#if e.phase === 'wiring'}<span class="size-3 shrink-0"></span>{:else if e.kept}<Check class="size-3 shrink-0 text-primary" />{:else}<X class="size-3 shrink-0 opacity-50" />{/if}
							<span class="min-w-0 flex-1 truncate">{e.caption}</span>
							{#if e.n === studio.compareWith}<Pin class="size-3 shrink-0 text-primary" aria-label="picked to compare" />{/if}
							{#if e.score !== null}<span class="shrink-0">{fmt(e.from)} → {e.score}</span>{/if}
						</button>
					{:else}
						<div class="px-3 py-2 font-sans text-xs text-muted-foreground">Each step bbgen takes shows here as it takes it.</div>
					{/each}
				</div>
			{/snippet}
		</ContextMenu.Trigger>
		<ContextMenu.Content class="w-60">
			{#if menuFor !== null}
				<ContextMenu.Label class="text-xs text-muted-foreground">Step {menuFor}</ContextMenu.Label>
				<ContextMenu.Item disabled={studio.compareWith === menuFor} onSelect={() => (studio.compareWith = menuFor)}>
					<Pin /> {studio.compareWith === menuFor ? 'Selected for compare' : 'Select for compare'}
				</ContextMenu.Item>
				<ContextMenu.Item disabled={studio.compareWith === null || studio.compareWith === menuFor}
					onSelect={() => menuFor !== null && studio.compareSteps(menuFor)}>
					<GitCompareArrows /> Compare with selected{#if studio.compareWith !== null && studio.compareWith !== menuFor}&nbsp;(step {studio.compareWith}){/if}
				</ContextMenu.Item>
			{/if}
		</ContextMenu.Content>
	</ContextMenu.Root>
</div>
