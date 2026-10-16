<script lang="ts">
	// The circuit view: the schematic bbgen draws (`bbgen build --schematic`), in a Drawing like the board, lit from
	// the same hover and selection: a net or part lights in both, a pin here lights its hole and strip there.
	import { studio } from '#lib/studio.svelte.js';
	import { circuitSelectors } from '#lib/lookup.js';
	import { hasSchematic, type Key } from '#lib/scene.js';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import Drawing from './Drawing.svelte';
	import BuildError from './BuildError.svelte';
	import { Toggle } from '#lib/components/ui/toggle/index.js';
	import TagX from '@lucide/svelte/icons/tag-x';

	const scene = $derived(studio.scene);
	const schematic = $derived(hasSchematic(scene) ? scene.schematic : null);
	const failed = $derived(scene?.schematic && 'error' in scene.schematic ? scene.schematic.error : null);

	const selectors = (key: Key) => (scene && studio.index ? circuitSelectors(key, scene, studio.index) : []);

	/** what the pointer is over: a pin, a net's wire, label or power symbol, a symbol */
	function under(target: Element): Key | null {
		const el = target.closest('[data-pin],[data-net],[data-part]');
		const d = (el as HTMLElement | SVGElement | null)?.dataset;
		if (d?.pin) return `pin:${d.pin}`;
		if (d?.net) return `net:${d.net}`;
		if (d?.part) return `part:${d.part}`;
		return null;
	}
</script>

<div class="circuit relative h-full w-full overflow-hidden bg-muted/60 text-foreground">
	{#if schematic}
		<Drawing svg={schematic.svg} width={schematic.width} height={schematic.height} name="schematic" {selectors} {under} ink>
			<div class="absolute top-3 right-3 z-10 rounded-lg border bg-background/95 p-1 shadow-sm">
				<Toggle size="sm" pressed={studio.wires} onPressedChange={(on) => studio.setWires(on)}
					aria-label="Draw every net as a wire, supplies from one power symbol each, with no labels">
					<TagX /> No labels
				</Toggle>
			</div>
			{#if studio.error}
				<BuildError banner />
			{:else if schematic.problems.length}
				<div class="absolute right-3 bottom-3 left-3 z-10 flex items-start gap-3 rounded-lg border border-destructive/40 bg-background/95 px-4 py-3 text-sm shadow-md">
					<TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
					<div>
						<div class="font-medium">bbgen drew this schematic wrong (a bug): trust the board's check, not the drawing.</div>
						{#each schematic.problems as p}<div class="mt-0.5 font-mono text-xs text-muted-foreground">{p}</div>{/each}
					</div>
				</div>
			{/if}
		</Drawing>
	{:else}
		<div class="flex h-full items-center justify-center p-8 text-sm text-muted-foreground">
			{#if studio.error && !scene}
				<BuildError />
			{:else if failed}
				<div class="flex items-center gap-2"><TriangleAlert class="size-4 text-destructive" /> No schematic: {failed}</div>
			{:else if scene || studio.building}
				<div class="flex items-center gap-2"><LoaderCircle class="size-4 animate-spin" /> Drawing the schematic…</div>
			{:else}
				Open a circuit to see its schematic.
			{/if}
		</div>
	{/if}
</div>

<style>
	/* the schematic is drawn in currentColor: the theme's ink, on its background */
	.circuit :global(.drawing-svg) {
		color: var(--foreground);
	}
</style>
