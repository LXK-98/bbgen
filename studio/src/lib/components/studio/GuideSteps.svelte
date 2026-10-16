<script lang="ts">
	// Under the board in the build guide: every step, section by section, ticked when done. Clicking a step takes it
	// in hand; its box marks it done or not. What's done is kept for each circuit.
	import { tick } from 'svelte';
	import { studio } from '#lib/studio.svelte.js';
	import { stepKey } from '#lib/scene.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import Code from './Code.svelte';
	import Check from '@lucide/svelte/icons/check';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';

	const scene = $derived(studio.scene);
	const list = $derived(scene?.checklist ?? []);
	const at = $derived(studio.guideAt);
	const title = (key: string) => scene?.sections?.find((s) => s.key === key)?.title ?? key;

	// keep the step in hand in view
	let box = $state<HTMLDivElement>();
	$effect(() => {
		const i = at;
		if (!box) return;
		tick().then(() => box?.querySelector(`[data-i="${i}"]`)?.scrollIntoView({ block: 'nearest' }));
	});
</script>

<div class="flex h-full flex-col bg-background">
	<div class="flex h-10 shrink-0 items-center gap-3 border-b px-3 text-sm">
		<span class="font-medium">Steps</span>
		<span class="text-xs text-muted-foreground tabular-nums">{studio.doneCount} of {list.length} done</span>
		<Button variant="ghost" size="sm" class="ml-auto" disabled={!studio.doneCount} onclick={() => studio.resetGuide()}>
			<RotateCcw /> Start over
		</Button>
	</div>
	<div bind:this={box} class="min-h-0 flex-1 overflow-y-auto p-1.5">
		{#each list as s, i (s.item)}
			{@const done = studio.done.has(stepKey(s))}
			{#if list[i - 1]?.section !== s.section}
				<div class="px-2.5 pt-2 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{title(s.section)}</div>
			{/if}
			<div
				data-i={i}
				class="flex w-full items-baseline gap-3 rounded-md px-2.5 py-1 text-sm {i === at ? 'bg-primary/10' : 'hover:bg-muted'} {done && i !== at ? 'text-muted-foreground' : ''}"
			>
				<button
					type="button"
					role="checkbox"
					aria-checked={done}
					aria-label="{s.item.split(':')[1]} done"
					onclick={() => studio.setDone(i, !done)}
					class="flex size-4 shrink-0 translate-y-0.5 items-center justify-center self-start rounded border {done ? 'border-primary bg-primary text-primary-foreground' : 'border-input bg-background'}"
				>
					{#if done}<Check class="size-3" />{/if}
				</button>
				<button type="button" class="flex min-w-0 flex-1 items-baseline gap-3 text-left" onclick={() => studio.guideGo(i)}>
					<span class="w-6 shrink-0 text-right font-mono text-[11px] text-muted-foreground">{i + 1}</span>
					<span class="w-11 shrink-0 font-mono text-xs font-semibold">{s.item.split(':')[1]}</span>
					<span class="min-w-0 flex-1"><Code text={s.text} /></span>
				</button>
			</div>
		{/each}
	</div>
</div>
