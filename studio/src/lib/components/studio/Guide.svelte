<script lang="ts">
	// The build guide: the board as it stands after the steps before the one in hand, what's not in yet drawn dashed,
	// the step in hand glowing with its holes tagged in large type, and the step under the board with Back and Done.
	// The board glides to each step (Follow). In the bench view this fills the screen, in larger type.
	import { untrack } from 'svelte';
	import { studio } from '#lib/studio.svelte.js';
	import { holeAt, selectorsFor } from '#lib/lookup.js';
	import { stepTags } from '#lib/guide.js';
	import { stepKey, type Key } from '#lib/scene.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Toggle } from '#lib/components/ui/toggle/index.js';
	import { Kbd } from '#lib/components/ui/kbd/index.js';
	import Drawing from './Drawing.svelte';
	import BuildError from './BuildError.svelte';
	import Code from './Code.svelte';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import Check from '@lucide/svelte/icons/check';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import Crosshair from '@lucide/svelte/icons/crosshair';
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';

	let { bench = false }: { bench?: boolean } = $props();

	const scene = $derived(studio.scene);
	const list = $derived(scene?.checklist ?? []);
	const at = $derived(studio.guideAt);
	const step = $derived(list[at] ?? null);
	const section = $derived(scene?.sections?.find((s) => s.key === step?.section) ?? null);
	/** the step in hand is the first of its section */
	const opens = $derived(!!step && list[at - 1]?.section !== step.section);
	const isDone = $derived(!!step && studio.done.has(stepKey(step)));
	const ref = $derived(step ? step.item.slice(step.item.indexOf(':') + 1) : '');

	/** what's in, what's in hand and what's not in yet: the steps before the one in hand are in */
	const marks = $derived.by(() => {
		const m = new Map<string, 'todo' | 'now'>();
		list.forEach((s, i) => {
			if (i === at) m.set(s.item, 'now');
			else if (i > at) m.set(s.item, 'todo');
		});
		return m;
	});
	const view = $derived(step && scene && studio.index ? stepTags(step, scene, studio.index) : { tags: [], box: null });

	let drawing = $state<ReturnType<typeof Drawing>>();
	// glide to each step as it's taken in hand (and once the board is there)
	let focused = '';
	$effect(() => {
		const box = view.box;
		const follow = studio.follow;
		const d = drawing;
		const key = `${studio.path}\n${step?.item}\n${box}`;
		if (!box || !follow || !d || key === focused) return;
		focused = key;
		untrack(() => d.focus(box, bench ? 2.5 : 2));
	});
	// the inspector shows the step in hand
	$effect(() => {
		const item = step?.item;
		if (item) untrack(() => (studio.selected = item as Key));
	});

	const selectors = (key: Key) => (scene && studio.index ? selectorsFor(key, scene, studio.index) : []);
	function under(target: Element, x: number, y: number): Key | null {
		studio.hole = studio.index ? holeAt(studio.index, x, y) : null;
		const el = target.closest('[data-wire],[data-part],[data-issue]');
		if (el) {
			const d = (el as SVGElement).dataset;
			if (d.wire) return `wire:${d.wire}`;
			if (d.part) return `part:${d.part}`;
			if (d.issue) return `issue:${Number(d.issue)}`;
		}
		return studio.hole ? `hole:${studio.hole}` : null;
	}

	/** a tag's size on screen, in pixels: large enough to read from the bench */
	const FONT = $derived(bench ? 22 : 16);

	/** how wide a tag's text is on screen, in the tags' font */
	let ctx: CanvasRenderingContext2D | null = null;
	function textWidth(label: string, sub: string | null): number {
		ctx ??= document.createElement('canvas').getContext('2d');
		const family = getComputedStyle(document.documentElement).getPropertyValue('--font-mono').trim() || 'monospace';
		if (!ctx) return (label.length + (sub ? sub.length + 1 : 0)) * FONT * 0.65;
		ctx.font = `600 ${FONT}px ${family}`;
		let w = ctx.measureText(label).width;
		if (sub) {
			ctx.font = `400 ${FONT}px ${family}`;
			w += ctx.measureText(` ${sub}`).width;
		}
		return w;
	}

	function onkeydown(e: KeyboardEvent) {
		const t = e.target as Element | null;
		if (t?.closest('input, textarea, [role=dialog], [role=menu]')) return;
		const onControl = !!t?.closest('button, a, [role=checkbox]');
		if (e.key === 'ArrowRight' || e.key === 'PageDown' || ((e.key === ' ' || e.key === 'Enter') && !onControl)) {
			e.preventDefault();
			studio.guideNext();
		} else if (e.key === 'ArrowLeft' || e.key === 'PageUp' || (e.key === 'Backspace' && !onControl)) {
			e.preventDefault();
			studio.guideBack();
		} else if (e.key === 'b' && !e.ctrlKey && !e.metaKey) {
			studio.setBench(!studio.bench);
		} else if (e.key === 'Escape' && studio.bench) {
			studio.setBench(false);
		}
	}
</script>

<svelte:window {onkeydown} />
<!-- a browser leaves full screen on its own Esc: leave the bench view with it -->
<svelte:document onfullscreenchange={() => !document.fullscreenElement && bench && studio.setBench(false)} />

<div class="flex h-full flex-col bg-muted/60">
	<div class="relative min-h-0 flex-1">
		{#if scene}
			<Drawing bind:this={drawing} svg={scene.svg} width={scene.width} height={scene.height} name="board" {selectors} {under}
				onleave={() => (studio.hole = null)} quiet={(k) => k.startsWith('hole:')} {marks}>
				{#snippet overlay(scale)}
					{@const fs = FONT / scale}
					<svg class="pointer-events-none absolute inset-0 overflow-visible" width={scene.width} height={scene.height} aria-hidden="true">
						{#each view.tags as t}
							{@const w = (textWidth(t.label, t.sub) + FONT * 0.9) / scale}
							{@const h = fs * 1.5}
							{@const gap = 9 + 6 / scale}
							{@const cx = t.xy[0] + t.dir[0] * (gap + w / 2)}
							{@const cy = t.xy[1] + t.dir[1] * (gap + h / 2)}
							<circle cx={t.xy[0]} cy={t.xy[1]} r="8" class="fill-primary/20 stroke-primary" stroke-width={2.5 / scale} />
							<line x1={t.xy[0] + t.dir[0] * 8} y1={t.xy[1] + t.dir[1] * 8} x2={t.xy[0] + t.dir[0] * gap} y2={t.xy[1] + t.dir[1] * gap}
								class="stroke-primary" stroke-width={2 / scale} />
							<rect x={cx - w / 2} y={cy - h / 2} width={w} height={h} rx={h / 4} class="fill-primary" />
							<text x={cx} y={cy} font-size={fs} text-anchor="middle" dominant-baseline="central"
								class="fill-primary-foreground font-mono font-semibold" xml:space="preserve">{t.label}{#if t.sub}<tspan font-weight="normal" opacity="0.8">{' ' + t.sub}</tspan>{/if}</text>
						{/each}
					</svg>
				{/snippet}

				<div class="absolute top-3 right-3 z-10 flex items-center gap-1 rounded-lg border bg-background/95 p-1 shadow-sm">
					<Toggle size="sm" pressed={studio.follow} onPressedChange={(on) => studio.setFollow(on)} aria-label="Zoom to each step">
						<Crosshair /> Follow
					</Toggle>
					<Button variant="ghost" size="sm" onclick={() => studio.setBench(!bench)}>
						{#if bench}<Minimize2 /> Leave bench view <Kbd>Esc</Kbd>{:else}<Maximize2 /> Bench view <Kbd>B</Kbd>{/if}
					</Button>
				</div>

				{#if studio.building}
					<div class="absolute top-3 left-1/2 z-10 flex -translate-x-1/2 items-center gap-2 rounded-lg border bg-background/95 px-3 py-1.5 text-xs shadow-sm">
						<LoaderCircle class="size-3.5 animate-spin" /> Building…
					</div>
				{/if}
				<BuildError banner />
			</Drawing>
		{:else}
			<div class="flex h-full items-center justify-center p-8 text-sm text-muted-foreground">
				{#if studio.building}
					<div class="flex items-center gap-2"><LoaderCircle class="size-4 animate-spin" /> Building…</div>
				{:else if studio.error}
					<BuildError />
				{:else}
					Open a circuit to build it step by step.
				{/if}
			</div>
		{/if}
	</div>

	{#if step}
		<div class="shrink-0 border-t bg-background">
			<!-- how far the build is: the steps done -->
			<div class="h-1 bg-muted" role="progressbar" aria-label="Steps done" aria-valuemin={0} aria-valuemax={list.length} aria-valuenow={studio.doneCount}>
				<div class="h-full bg-primary transition-[width]" style:width="{(100 * studio.doneCount) / list.length}%"></div>
			</div>
			<div class="flex items-center gap-4 {bench ? 'px-6 py-5' : 'px-4 py-3'}">
				<div class="shrink-0 {bench ? 'w-44' : 'w-32'}">
					<div class="font-mono tabular-nums {bench ? 'text-lg' : 'text-xs'} text-muted-foreground">Step {at + 1} of {list.length}</div>
					<div class="font-medium {bench ? 'text-base' : 'text-xs'}">{section?.title ?? step.section}</div>
				</div>
				<div class="min-w-0 flex-1 {bench ? 'text-2xl leading-snug' : 'text-sm'}">
					<div>
						{#if isDone}<CircleCheck class="mr-1 inline {bench ? 'size-6' : 'size-4'} -translate-y-px text-emerald-600" aria-label="done" />{/if}
						<span class="font-mono font-semibold">{ref}</span>
						<Code text={step.text} />
					</div>
					{#if opens && section}
						<div class="mt-1 {bench ? 'text-base' : 'text-xs'} text-muted-foreground">{section.intro}</div>
					{/if}
					{#if scene?.stats.errors && at === 0}
						<div class="mt-1 flex items-center gap-1.5 text-xs text-destructive">
							<TriangleAlert class="size-3.5" /> The check found {scene.stats.errors} {scene.stats.errors === 1 ? 'error' : 'errors'}: fix the circuit before building it.
						</div>
					{/if}
				</div>
				<div class="flex shrink-0 items-center gap-2">
					<Button variant="outline" size={bench ? 'lg' : 'default'} disabled={at === 0} onclick={() => studio.guideBack()}>
						<ChevronLeft /> Back
					</Button>
					<Button size={bench ? 'lg' : 'default'} class={bench ? 'min-w-44' : 'min-w-32'} onclick={() => studio.guideNext()}>
						<Check /> {at < list.length - 1 ? 'Done, next' : 'Done'}
						<Kbd class="ml-1 bg-primary-foreground/20 text-primary-foreground">→</Kbd>
					</Button>
				</div>
			</div>
		</div>
	{/if}
</div>
