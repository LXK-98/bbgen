<script lang="ts">
	// The board: bbgen's own drawing, put in the page once per build. Hover and selection only toggle classes on
	// its `data-part` / `data-wire` / ... groups, so nothing is redrawn as the pointer moves.
	import { studio } from '#lib/studio.svelte.js';
	import { holeAt, selectorsFor } from '#lib/lookup.js';
	import { describe } from '#lib/describe.js';
	import type { Key } from '#lib/scene.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Toggle } from '#lib/components/ui/toggle/index.js';
	import ZoomIn from '@lucide/svelte/icons/zoom-in';
	import ZoomOut from '@lucide/svelte/icons/zoom-out';
	import Maximize from '@lucide/svelte/icons/maximize';
	import Contrast from '@lucide/svelte/icons/contrast';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import Legend from './Legend.svelte';

	let viewport = $state<HTMLDivElement>();
	let content = $state<HTMLDivElement>();
	let scale = $state(1);
	let tx = $state(0);
	let ty = $state(0);
	let pointer = $state<{ x: number; y: number } | null>(null);
	let fittedFor: string | null = null;
	// a press: where it started, and what was under it (once the board captures the pointer, events target the board)
	let drag: { x: number; y: number; tx: number; ty: number; moved: boolean; key: Key | null } | null = null;

	const scene = $derived(studio.scene);
	const tip = $derived(
		studio.hover && scene && studio.index && pointer ? describe(studio.hover, scene, studio.index) : null
	);

	/** the strip of the hole under the pointer, outlined */
	const strip = $derived.by(() => {
		const ix = studio.index;
		if (!ix || !studio.hole) return null;
		const s = ix.strip.get(ix.stripOf.get(studio.hole) ?? '');
		if (!s) return null;
		const pts = s.holes.map((h) => ix.holes.find((x) => x.name === h)!.xy);
		const xs = pts.map((p) => p[0]);
		const ys = pts.map((p) => p[1]);
		const at = ix.holes.find((x) => x.name === studio.hole)!.xy;
		return { x0: Math.min(...xs) - 7, y0: Math.min(...ys) - 7, x1: Math.max(...xs) + 7, y1: Math.max(...ys) + 7, at };
	});

	export function fit() {
		if (!viewport || !scene) return;
		const { clientWidth: w, clientHeight: h } = viewport;
		scale = Math.min(w / scene.width, h / scene.height) * 0.96;
		tx = (w - scene.width * scale) / 2;
		ty = (h - scene.height * scale) / 2;
	}

	function zoomBy(k: number, cx?: number, cy?: number) {
		if (!viewport) return;
		const x = cx ?? viewport.clientWidth / 2;
		const y = cy ?? viewport.clientHeight / 2;
		const next = Math.min(8, Math.max(0.2, scale * k));
		tx = x - ((x - tx) * next) / scale;
		ty = y - ((y - ty) * next) / scale;
		scale = next;
	}

	// a new circuit is fitted to the view; a rebuild of the same one keeps where you were
	$effect(() => {
		if (scene && studio.path !== fittedFor && viewport) {
			fittedFor = studio.path;
			fit();
		}
	});

	// light what's hovered and selected
	$effect(() => {
		const s = scene;
		const ix = studio.index;
		const root = content;
		const hover = studio.hover;
		const selected = studio.selected;
		const dim = studio.dim;
		if (!s || !ix || !root) return;
		for (const el of root.querySelectorAll('.lit, .sel')) el.classList.remove('lit', 'sel');
		const mark = (key: Key, cls: string) => {
			for (const q of selectorsFor(key, s, ix)) for (const el of root.querySelectorAll(q)) el.classList.add(cls);
		};
		if (selected) mark(selected, 'sel');
		if (hover) mark(hover, 'lit');
		root.classList.toggle('dim', !!hover && dim && !hover.startsWith('hole:'));
	});

	/** what the pointer is over: a part, wire, issue ring or legend entry, else the hole */
	function under(e: PointerEvent): Key | null {
		const el = (e.target as Element).closest('[data-wire],[data-part],[data-issue],[data-net]');
		if (el) {
			const d = (el as HTMLElement | SVGElement).dataset;
			if (d.wire) return `wire:${d.wire}`;
			if (d.part) return `part:${d.part}`;
			if (d.issue) return `issue:${Number(d.issue)}`;
			if (d.net) return `net:${d.net}`;
		}
		return studio.hole ? `hole:${studio.hole}` : null;
	}

	function onmove(e: PointerEvent) {
		if (!viewport || !content || !studio.index) return;
		const box = viewport.getBoundingClientRect();
		pointer = { x: e.clientX - box.left, y: e.clientY - box.top };
		if (drag) {
			const dx = e.clientX - drag.x;
			const dy = e.clientY - drag.y;
			if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
			tx = drag.tx + dx;
			ty = drag.ty + dy;
			return;
		}
		studio.hole = holeAt(studio.index, (pointer.x - tx) / scale, (pointer.y - ty) / scale);
		studio.hover = under(e);
	}

	function ondown(e: PointerEvent) {
		if (e.button !== 0 && e.button !== 1) return;
		drag = { x: e.clientX, y: e.clientY, tx, ty, moved: false, key: under(e) };
		viewport?.setPointerCapture(e.pointerId);
	}

	function onup(e: PointerEvent) {
		const press = drag;
		drag = null;
		if (press && !press.moved && e.button === 0) studio.select(press.key);
	}

	function onleave() {
		pointer = null;
		studio.hole = null;
		if (studio.hover?.startsWith('hole:')) studio.hover = null;
		else if (!drag) studio.hover = null;
	}

	function onwheel(e: WheelEvent) {
		e.preventDefault();
		const box = viewport!.getBoundingClientRect();
		zoomBy(Math.exp(-e.deltaY * 0.0015), e.clientX - box.left, e.clientY - box.top);
	}

	function onkey(e: KeyboardEvent) {
		if ((e.target as HTMLElement).closest('input, textarea')) return;
		if (e.key === 'f') fit();
		else if (e.key === '+' || e.key === '=') zoomBy(1.25);
		else if (e.key === '-') zoomBy(0.8);
		else if (e.key === 'Escape') studio.select(null);
	}
</script>

<svelte:window onkeydown={onkey} />

<div class="relative h-full w-full overflow-hidden bg-muted/60">
	{#if scene}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			bind:this={viewport}
			class="absolute inset-0 cursor-default touch-none select-none"
			onpointermove={onmove}
			onpointerdown={ondown}
			onpointerup={onup}
			onpointerleave={onleave}
			onwheel={onwheel}
		>
			<div
				class="board absolute top-0 left-0 origin-top-left"
				style:width="{scene.width}px"
				style:height="{scene.height}px"
				style:transform="translate({tx}px, {ty}px) scale({scale})"
			>
				<div bind:this={content} class="board-svg">{@html scene.svg}</div>
				{#if strip}
					<svg class="pointer-events-none absolute inset-0" width={scene.width} height={scene.height} aria-hidden="true">
						<rect
							x={strip.x0}
							y={strip.y0}
							width={strip.x1 - strip.x0}
							height={strip.y1 - strip.y0}
							rx="6"
							class="fill-primary/10 stroke-primary"
							stroke-width={1.5 / scale}
						/>
						<circle cx={strip.at[0]} cy={strip.at[1]} r="6" class="fill-none stroke-primary" stroke-width={2 / scale} />
					</svg>
				{/if}
			</div>
		</div>

		{#if tip && pointer}
			<div
				class="pointer-events-none absolute z-20 max-w-72 rounded-lg border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md"
				style:left="{Math.min(pointer.x + 16, (viewport?.clientWidth ?? 0) - 290)}px"
				style:top="{pointer.y + 18}px"
			>
				<div class="flex items-center gap-2 font-medium">
					{#if tip.color}<span class="size-2.5 shrink-0 rounded-sm" style:background={tip.color}></span>{/if}
					<span class="font-mono">{tip.title}</span>
				</div>
				{#each tip.lines as line}<div class="mt-0.5 text-muted-foreground">{line}</div>{/each}
			</div>
		{/if}

		<div class="absolute top-3 left-3 z-10 flex items-center gap-1 rounded-lg border bg-background/95 p-1 shadow-sm">
			<Button variant="ghost" size="icon-sm" aria-label="Zoom out" onclick={() => zoomBy(0.8)}><ZoomOut /></Button>
			<span class="w-11 text-center font-mono text-xs tabular-nums">{Math.round(scale * 100)}%</span>
			<Button variant="ghost" size="icon-sm" aria-label="Zoom in" onclick={() => zoomBy(1.25)}><ZoomIn /></Button>
			<Button variant="ghost" size="icon-sm" aria-label="Fit the board" onclick={fit}><Maximize /></Button>
			<div class="mx-1 h-5 w-px bg-border"></div>
			<Toggle size="sm" bind:pressed={studio.dim} aria-label="Dim the rest on hover">
				<Contrast /> Dim the rest
			</Toggle>
		</div>

		<Legend />

		{#if studio.building}
			<div class="absolute top-3 left-1/2 z-10 flex -translate-x-1/2 items-center gap-2 rounded-lg border bg-background/95 px-3 py-1.5 text-xs shadow-sm">
				<LoaderCircle class="size-3.5 animate-spin" /> Building…
			</div>
		{/if}

		{#if studio.error}
			<div class="absolute right-3 bottom-3 left-3 z-10 flex items-start gap-3 rounded-lg border border-destructive/40 bg-background/95 px-4 py-3 text-sm shadow-md">
				<TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
				<div>
					<div class="font-medium">The latest save didn't build; this is the last board that did.</div>
					<div class="mt-0.5 font-mono text-xs text-muted-foreground">
						{studio.error.message}{#if studio.error.line && !studio.error.message.includes('(line')} · line {studio.error.line}{/if}
					</div>
				</div>
			</div>
		{/if}
	{:else}
		<div class="flex h-full items-center justify-center p-8">
			{#if studio.building}
				<div class="flex items-center gap-2 text-sm text-muted-foreground">
					<LoaderCircle class="size-4 animate-spin" /> Building {studio.path?.split('/').pop()}…
				</div>
			{:else if studio.error}
				<div class="max-w-lg rounded-xl border border-destructive/40 bg-background p-5 text-sm">
					<div class="flex items-center gap-2 font-medium"><TriangleAlert class="size-4 text-destructive" /> bbgen couldn't build this circuit</div>
					<pre class="mt-3 font-mono text-xs whitespace-pre-wrap text-muted-foreground">{studio.error.message}{#if studio.error.line}
line {studio.error.line}, column {studio.error.column}{/if}</pre>
				</div>
			{:else}
				<div class="text-center">
					<div class="text-sm font-medium">No circuit open</div>
					<p class="mt-1 text-sm text-muted-foreground">Open a circuit file to build its breadboard.</p>
					<Button class="mt-4" onclick={() => (studio.openDialog = true)}>Open a circuit…</Button>
				</div>
			{/if}
		</div>
	{/if}
</div>

<style>
	.board-svg :global(svg) {
		display: block;
	}
	.board-svg :global([data-part]),
	.board-svg :global([data-wire]),
	.board-svg :global([data-issue]),
	.board-svg :global([data-net]) {
		transition: opacity 120ms;
	}
	:global(.board-svg.dim [data-part]:not(.lit)),
	:global(.board-svg.dim [data-wire]:not(.lit)),
	:global(.board-svg.dim [data-issue]:not(.lit)),
	:global(.board-svg.dim [data-net]:not(.lit)) {
		opacity: 0.16;
	}
	.board-svg :global(.sel) {
		filter: drop-shadow(0 0 1.5px var(--primary)) drop-shadow(0 0 3px var(--primary));
	}
	.board-svg :global(.lit) {
		filter: drop-shadow(0 0 1px #fff) drop-shadow(0 0 2.5px var(--primary));
	}
</style>
