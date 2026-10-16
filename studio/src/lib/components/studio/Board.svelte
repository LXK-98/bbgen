<script lang="ts">
	// The board: bbgen's own drawing, in a Drawing (pan, zoom, hover, selection), with the hole under the pointer and
	// its strip outlined, and the legend.
	import { studio } from '#lib/studio.svelte.js';
	import { holeAt, selectorsFor } from '#lib/lookup.js';
	import type { Key } from '#lib/scene.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import Drawing from './Drawing.svelte';
	import Legend from './Legend.svelte';

	const scene = $derived(studio.scene);

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

	const selectors = (key: Key) => (scene && studio.index ? selectorsFor(key, scene, studio.index) : []);

	/** what the pointer is over: a part, wire, issue ring or legend entry, else the hole */
	function under(target: Element, x: number, y: number): Key | null {
		studio.hole = studio.index ? holeAt(studio.index, x, y) : null;
		const el = target.closest('[data-wire],[data-part],[data-issue],[data-net]');
		if (el) {
			const d = (el as HTMLElement | SVGElement).dataset;
			if (d.wire) return `wire:${d.wire}`;
			if (d.part) return `part:${d.part}`;
			if (d.issue) return `issue:${Number(d.issue)}`;
			if (d.net) return `net:${d.net}`;
		}
		return studio.hole ? `hole:${studio.hole}` : null;
	}

	function onleave() {
		studio.hole = null;
	}
</script>

<div class="relative h-full w-full overflow-hidden bg-muted/60">
	{#if scene}
		<Drawing svg={scene.svg} width={scene.width} height={scene.height} name="board" {selectors} {under} {onleave}
			quiet={(k) => k.startsWith('hole:')}>
			{#snippet overlay(scale)}
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
			{/snippet}
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
		</Drawing>
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
