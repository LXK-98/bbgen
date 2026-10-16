<script lang="ts">
	// One of bbgen's drawings (the board, or the circuit's schematic), put in the page once per build, with pan and
	// zoom. Hover and selection only toggle classes on its `data-*` groups, so nothing is redrawn as the pointer
	// moves; what a key lights up is the caller's `selectors`, and what's under the pointer its `under`.
	import type { Snippet } from 'svelte';
	import { studio } from '#lib/studio.svelte.js';
	import { describe } from '#lib/describe.js';
	import type { Key } from '#lib/scene.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Toggle } from '#lib/components/ui/toggle/index.js';
	import ZoomIn from '@lucide/svelte/icons/zoom-in';
	import ZoomOut from '@lucide/svelte/icons/zoom-out';
	import Maximize from '@lucide/svelte/icons/maximize';
	import Contrast from '@lucide/svelte/icons/contrast';

	interface Props {
		svg: string;
		width: number;
		height: number;
		/** what it shows, for the fit button */
		name: string;
		/** what a key lights up, as CSS selectors over the drawing's groups */
		selectors: (key: Key) => string[];
		/** what's under the pointer: the element it's over, and the point in the drawing's pixels */
		under: (target: Element, x: number, y: number) => Key | null;
		/** the pointer left the drawing */
		onleave?: () => void;
		/** keys that don't dim the rest when hovered (a hole on the board) */
		quiet?: (key: Key) => boolean;
		/** drawn over the drawing, in its pixels; given the zoom */
		overlay?: Snippet<[number]>;
		/** over the view: a legend, banners */
		children?: Snippet;
		/** another drawing of the same size to compare with: shown left of a divider you drag, this one right of it */
		before?: string;
		/** what each side of the divider shows */
		beforeLabel?: string;
		afterLabel?: string;
		/** the minimap's drawing is in currentColor (the schematic): an image draws that black, so invert it in the dark */
		ink?: boolean;
		class?: string;
	}

	let { svg, width, height, name, selectors, under, onleave, quiet, overlay, children, ink = false, before, beforeLabel, afterLabel,
		class: klass = '' }: Props = $props();

	let viewport = $state<HTMLDivElement>();
	let content = $state<HTMLDivElement>();
	let scale = $state(1);
	let tx = $state(0);
	let ty = $state(0);
	let pointer = $state<{ x: number; y: number } | null>(null);
	// the view's size, for the minimap
	let vw = $state(0);
	let vh = $state(0);
	let fittedFor: string | null = null;
	// a press: where it started, and what was under it (once the view captures the pointer, events target the view)
	let drag: { x: number; y: number; tx: number; ty: number; moved: boolean; key: Key | null } | null = null;

	const tip = $derived(
		studio.hover && studio.scene && studio.index && pointer ? describe(studio.hover, studio.scene, studio.index) : null
	);

	export function fit() {
		if (!viewport) return;
		const { clientWidth: w, clientHeight: h } = viewport;
		scale = Math.min(w / width, h / height) * 0.96;
		tx = (w - width * scale) / 2;
		ty = (h - height * scale) / 2;
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
		if (studio.path !== fittedFor && viewport && width) {
			fittedFor = studio.path;
			fit();
		}
	});

	// light what's hovered and selected
	$effect(() => {
		const root = content;
		const hover = studio.hover;
		const selected = studio.selected;
		const dim = studio.dim;
		void svg;
		if (!root) return;
		for (const el of root.querySelectorAll('.lit, .sel')) el.classList.remove('lit', 'sel');
		const mark = (key: Key, cls: string) => {
			for (const q of selectors(key)) for (const el of root.querySelectorAll(q)) el.classList.add(cls);
		};
		if (selected) mark(selected, 'sel');
		if (hover) mark(hover, 'lit');
		root.classList.toggle('dim', !!hover && dim && !quiet?.(hover));
	});

	// the minimap: the drawing at a fixed size (an image, made once per build), the visible part outlined
	const MINI = { w: 180, h: 120 };
	const mini = $derived(Math.min(MINI.w / width, MINI.h / height));
	const miniSrc = $derived(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`);
	const seen = $derived.by(() => {
		const x0 = Math.max(0, -tx / scale);
		const y0 = Math.max(0, -ty / scale);
		const x1 = Math.min(width, (vw - tx) / scale);
		const y1 = Math.min(height, (vh - ty) / scale);
		return { x0, y0, x1, y1, all: x0 <= 0 && y0 <= 0 && x1 >= width && y1 >= height };
	});
	let miniDrag = false;

	// comparing: where the divider is, as a share of the view's width
	let split = $state(0.5);
	let splitting = false;
	const splitX = $derived(Math.round(split * vw));
	function splitMove(e: PointerEvent) {
		const box = viewport!.getBoundingClientRect();
		split = Math.min(1, Math.max(0, (e.clientX - box.left) / box.width));
	}

	/** centre the view on the drawing's point under the pointer on the minimap */
	function miniMove(e: PointerEvent) {
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		tx = vw / 2 - ((e.clientX - box.left) / mini) * scale;
		ty = vh / 2 - ((e.clientY - box.top) / mini) * scale;
	}

	function onmove(e: PointerEvent) {
		if (!viewport) return;
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
		studio.hover = under(e.target as Element, (pointer.x - tx) / scale, (pointer.y - ty) / scale);
	}

	function ondown(e: PointerEvent) {
		if (e.button !== 0 && e.button !== 1) return;
		const box = viewport!.getBoundingClientRect();
		const key = under(e.target as Element, (e.clientX - box.left - tx) / scale, (e.clientY - box.top - ty) / scale);
		drag = { x: e.clientX, y: e.clientY, tx, ty, moved: false, key };
		viewport?.setPointerCapture(e.pointerId);
	}

	function onup(e: PointerEvent) {
		const press = drag;
		drag = null;
		if (press && !press.moved && e.button === 0) studio.select(press.key);
	}

	function leave() {
		pointer = null;
		onleave?.();
		if (!drag) studio.hover = null;
	}

	function onwheel(e: WheelEvent) {
		e.preventDefault();
		const box = viewport!.getBoundingClientRect();
		zoomBy(Math.exp(-e.deltaY * 0.0015), e.clientX - box.left, e.clientY - box.top);
	}

	// keys work in the view the pointer is over (with the board and circuit side by side, each has its own zoom)
	let hovered = false;
	function onkey(e: KeyboardEvent) {
		if (!hovered || (e.target as HTMLElement).closest('input, textarea')) return;
		if (e.key === 'f') fit();
		else if (e.key === '+' || e.key === '=') zoomBy(1.25);
		else if (e.key === '-') zoomBy(0.8);
	}
</script>

<svelte:window onkeydown={onkey} />

<div
	class="relative h-full w-full overflow-hidden {klass}"
	role="presentation"
	onpointerenter={() => (hovered = true)}
	onpointerleave={() => (hovered = false)}
>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		bind:this={viewport}
		bind:clientWidth={vw}
		bind:clientHeight={vh}
		class="absolute inset-0 cursor-default touch-none select-none"
		onpointermove={onmove}
		onpointerdown={ondown}
		onpointerup={onup}
		onpointerleave={leave}
		onwheel={onwheel}
	>
		<div
			class="absolute top-0 left-0 origin-top-left"
			style:width="{width}px"
			style:height="{height}px"
			style:transform="translate({tx}px, {ty}px) scale({scale})"
		>
			<div bind:this={content} class="drawing-svg">{@html svg}</div>
			{@render overlay?.(scale)}
		</div>
		{#if before !== undefined}
			<div class="pointer-events-none absolute inset-0 overflow-hidden" style:clip-path="inset(0 {vw - splitX}px 0 0)">
				<div
					class="absolute top-0 left-0 origin-top-left"
					style:width="{width}px"
					style:height="{height}px"
					style:transform="translate({tx}px, {ty}px) scale({scale})"
				>
					<div class="drawing-svg">{@html before}</div>
				</div>
			</div>
		{/if}
	</div>

	{#if before !== undefined && vw}
		<!-- the divider: drag it (or anywhere on its line) to show more of either drawing -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="absolute top-0 bottom-0 z-[5] w-4 -translate-x-1/2 cursor-ew-resize touch-none"
			style:left="{splitX}px"
			onpointerdown={(e) => {
				e.stopPropagation();
				splitting = true;
				(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
			}}
			onpointermove={(e) => splitting && splitMove(e)}
			onpointerup={() => (splitting = false)}
		>
			<div class="absolute top-0 bottom-0 left-1/2 w-0.5 -translate-x-1/2 bg-primary shadow-[0_0_0_1px_var(--background)]"></div>
			<div class="absolute top-1/2 left-1/2 flex h-9 w-6 -translate-1/2 items-center justify-center rounded-md border-2 border-primary bg-background text-primary shadow-md">
				<svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true"><path d="M4 2 0 6l4 4zM8 2l4 4-4 4z" /></svg>
			</div>
		</div>
		{#if beforeLabel}
			<div class="pointer-events-none absolute bottom-3 z-[6] -translate-x-full pr-3 whitespace-nowrap" style:left="{splitX}px">
				<span class="rounded-md border bg-background/95 px-2 py-1 text-xs font-medium shadow-sm">{beforeLabel}</span>
			</div>
		{/if}
		{#if afterLabel}
			<div class="pointer-events-none absolute bottom-3 z-[6] pl-3 whitespace-nowrap" style:left="{splitX}px">
				<span class="rounded-md border bg-background/95 px-2 py-1 text-xs font-medium shadow-sm">{afterLabel}</span>
			</div>
		{/if}
	{/if}

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
		<Button variant="ghost" size="icon-sm" aria-label="Fit the {name}" onclick={fit}><Maximize /></Button>
		<div class="mx-1 h-5 w-px bg-border"></div>
		<Toggle size="sm" bind:pressed={studio.dim} aria-label="Dim the rest on hover">
			<Contrast /> Dim the rest
		</Toggle>
	</div>

	{#if !seen.all && vw}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="absolute right-3 z-10 cursor-move overflow-hidden rounded-lg border bg-background/95 shadow-sm touch-none {studio.error ? 'bottom-24' : 'bottom-3'}"
			style:width="{width * mini}px"
			style:height="{height * mini}px"
			title="Drag to move the view"
			onpointerdown={(e) => {
				miniDrag = true;
				(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
				miniMove(e);
			}}
			onpointermove={(e) => miniDrag && miniMove(e)}
			onpointerup={() => (miniDrag = false)}
		>
			<img src={miniSrc} alt="" draggable="false" class="pointer-events-none block size-full {ink ? 'dark:invert' : ''}" />
			<div
				class="pointer-events-none absolute rounded-sm border-2 border-primary bg-primary/10"
				style:left="{seen.x0 * mini}px"
				style:top="{seen.y0 * mini}px"
				style:width="{Math.max(4, (seen.x1 - seen.x0) * mini)}px"
				style:height="{Math.max(4, (seen.y1 - seen.y0) * mini)}px"
			></div>
		</div>
	{/if}

	{@render children?.()}
</div>

<style>
	.drawing-svg :global(svg) {
		display: block;
	}
	.drawing-svg :global([data-part]),
	.drawing-svg :global([data-wire]),
	.drawing-svg :global([data-issue]),
	.drawing-svg :global([data-net]) {
		transition: opacity 120ms;
	}
	:global(.drawing-svg.dim [data-part]:not(.lit)),
	:global(.drawing-svg.dim [data-wire]:not(.lit)),
	:global(.drawing-svg.dim [data-issue]:not(.lit)),
	:global(.drawing-svg.dim [data-net]:not(.lit)) {
		opacity: 0.16;
	}
	/* a lit pin inside a dimmed symbol stays lit */
	:global(.drawing-svg.dim [data-part]:has(.lit)) {
		opacity: 1;
	}
	.drawing-svg :global(.sel) {
		filter: drop-shadow(0 0 1.5px var(--primary)) drop-shadow(0 0 3px var(--primary));
	}
	.drawing-svg :global(.lit) {
		filter: drop-shadow(0 0 1px #fff) drop-shadow(0 0 2.5px var(--primary));
	}
</style>
