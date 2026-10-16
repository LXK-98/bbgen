<script lang="ts">
	// Generation mode: bbgen placing and wiring the board, step by step (`bbgen build --events`). The board as it
	// stood at the kept step shown (the latest while it runs), the parts and wires that step changed ringed; the
	// phases; a scrubber over the kept steps (← and →). The score chart and the log are under it (Progress.svelte).
	import { studio } from '#lib/studio.svelte.js';
	import type { StepEvent } from '#lib/scene.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import Drawing from './Drawing.svelte';
	import BuildError from './BuildError.svelte';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Play from '@lucide/svelte/icons/play';
	import Check from '@lucide/svelte/icons/check';

	const PHASES = [
		{ phase: 'start', label: 'Starting layouts' },
		{ phase: 'turn', label: 'Turning' },
		{ phase: 'repair', label: 'Moving parts' },
		{ phase: 'wiring', label: 'Wiring' }
	] as const;

	const steps = $derived(studio.ofFile);
	const kept = $derived(studio.kept);
	const at = $derived(studio.keptAt);
	const shown = $derived<StepEvent | null>(studio.shownStep);
	const live = $derived(studio.live);
	/** the phase bbgen is in (while it runs) or the shown step was in */
	const phase = $derived(live ? studio.step?.phase : shown?.phase);
	const phaseAt = $derived(PHASES.findIndex((p) => p.phase === phase));
	/** the phases this run went through: a placed file goes straight to wiring */
	const ran = $derived(new Set(steps.map((e) => e.phase)));
	const best = $derived(kept.filter((e) => e.score !== null && e.n <= (shown?.n ?? Infinity)).at(-1)?.score ?? null);
	const go = (i: number) => studio.showKept(i);

	function onkeydown(e: KeyboardEvent) {
		if (e.target instanceof Element && e.target.closest('input:not([type=range]), textarea, [role=dialog]')) return;
		const moves: Record<string, number> = { ArrowLeft: at - 1, ArrowRight: at + 1, Home: 0, End: kept.length - 1 };
		if (e.key in moves && kept.length) {
			e.preventDefault();
			go(moves[e.key]);
		}
	}

	const fmt = (s: number | null) => (s === null ? '–' : String(s));
</script>

<svelte:window {onkeydown} />

<div class="flex h-full flex-col bg-muted/60">
	<div class="relative min-h-0 flex-1">
		{#if shown?.svg}
			<Drawing svg={shown.svg} width={shown.width ?? 0} height={shown.height ?? 0} name="board at this step"
				selectors={() => []} under={() => null} quiet={() => true}>
				{#if studio.error}<BuildError banner />{/if}
			</Drawing>
		{:else}
			<div class="flex h-full items-center justify-center p-8 text-sm text-muted-foreground">
				{#if !studio.path}
					Open a circuit to watch bbgen place and wire it.
				{:else if studio.error && !studio.building}
					<BuildError />
				{:else if studio.stepping}
					<div class="flex items-center gap-2"><LoaderCircle class="size-4 animate-spin" /> bbgen is starting…</div>
				{:else if studio.stepsFor === studio.path}
					<div class="max-w-sm text-center">bbgen had nothing to place or wire: every part and wire is in the file.</div>
				{:else}
					<Button onclick={() => studio.rebuild(true)}><Play /> Watch bbgen build it</Button>
				{/if}
			</div>
		{/if}

		{#if steps.length}
			<div class="pointer-events-none absolute top-3 right-3 z-10 flex max-w-[calc(100%-22rem)] flex-col items-end gap-2">
				<ol class="flex w-fit items-center gap-0.5 rounded-lg border bg-background/95 p-1 text-xs shadow-sm" aria-label="Phases">
					{#each PHASES as p, i}
						<li
							aria-current={i === phaseAt ? 'step' : undefined}
							class="flex h-6 items-center gap-1.5 rounded-md px-2.5 {i === phaseAt
								? 'bg-primary font-medium text-primary-foreground'
								: i < phaseAt && ran.has(p.phase)
									? 'text-foreground'
									: 'text-muted-foreground/60'}"
						>
							{#if i === phaseAt && live}<LoaderCircle class="size-3 animate-spin" />{:else if i < phaseAt && ran.has(p.phase)}<Check class="size-3" />{/if}
							{p.label}
						</li>
					{/each}
				</ol>
				{#if shown}
					<div class="w-fit max-w-full truncate rounded-lg border bg-background/95 px-3 py-1.5 text-xs shadow-sm">
						<span class="text-muted-foreground">Step {shown.n}:</span>
						<span class="font-medium">{shown.caption}</span>
						{#if shown.score !== null}<span class="ml-1 font-mono text-muted-foreground">{fmt(shown.from)} → {shown.score}</span>{/if}
					</div>
				{/if}
			</div>
		{/if}
	</div>

	{#if kept.length}
		<div class="flex h-10 shrink-0 items-center gap-2 border-t bg-background px-3 text-xs">
			<Button variant="ghost" size="icon" class="size-7" disabled={at <= 0} onclick={() => go(at - 1)} aria-label="The kept step before">
				<ChevronLeft />
			</Button>
			<input type="range" min="0" max={kept.length - 1} value={at} oninput={(e) => go(Number(e.currentTarget.value))}
				aria-label="Kept step" class="min-w-0 flex-1 accent-primary" />
			<Button variant="ghost" size="icon" class="size-7" disabled={at >= kept.length - 1} onclick={() => go(at + 1)} aria-label="The kept step after">
				<ChevronRight />
			</Button>
			<span class="w-44 shrink-0 text-right whitespace-nowrap text-muted-foreground">
				{#if live}<span class="font-medium text-primary">live</span> · {/if}kept step {at + 1} of {kept.length}{#if best !== null} · best <span class="font-mono text-foreground">{best}</span>{/if}
			</span>
		</div>
	{/if}
</div>
