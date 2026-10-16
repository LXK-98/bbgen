<script lang="ts">
	// The nets' colours, in the top right of the board view: it stays put while the board pans and zooms. Hovering an
	// entry lights its net on the board; clicking selects it.
	import { studio } from '#lib/studio.svelte.js';
	import type { Key } from '#lib/scene.js';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';

	let open = $state(true);
	const scene = $derived(studio.scene);
	const flex = $derived(scene?.wires.some((w) => w.flex) ?? false);
</script>

{#if scene && scene.nets.length}
	<div class="absolute top-3 right-3 z-10 flex max-h-[calc(100%-1.5rem)] min-w-32 flex-col rounded-lg border bg-background/95 shadow-sm">
		<button class="flex w-full items-center gap-2 px-3 py-1.5 text-xs font-medium" onclick={() => (open = !open)} aria-expanded={open}>
			Legend <ChevronDown class="size-3.5 transition-transform {open ? '' : '-rotate-90'}" />
		</button>
		{#if open}
			<div class="flex min-h-0 flex-col overflow-y-auto px-1.5 pb-1.5">
				{#each scene.nets as n (n.name)}
					{@const key = `net:${n.name}` as Key}
					<button
						class="flex items-center gap-2 rounded px-1.5 py-1 text-left font-mono text-[11px] hover:bg-muted {studio.selected === key ? 'bg-primary/10' : ''}"
						{...studio.pointer(key)}
					>
						<span class="h-2 w-3.5 rounded-sm ring-1 ring-black/20 dark:ring-white/25" style:background={n.color}></span>{n.name}
					</button>
				{/each}
				{#if flex}
					<span class="flex items-center gap-2 px-1.5 py-1 text-[11px] text-muted-foreground">
						<svg width="16" height="9" viewBox="0 0 16 9" aria-hidden="true"><path d="M2 8 Q8 -2 14 8" fill="none" stroke="currentColor" stroke-width="2" /></svg>
						flexible jumper
					</span>
				{/if}
			</div>
		{/if}
	</div>
{/if}
