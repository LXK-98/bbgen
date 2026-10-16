<script lang="ts">
	// The circuit's parts, nets and wires as lists. Hovering a row lights it on the board; clicking selects it.
	import { studio } from '#lib/studio.svelte.js';
	import { partName } from '#lib/describe.js';
	import type { Key } from '#lib/scene.js';
	import * as Tabs from '#lib/components/ui/tabs/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { ScrollArea } from '#lib/components/ui/scroll-area/index.js';
	import Search from '@lucide/svelte/icons/search';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';

	const scene = $derived(studio.scene);
	const q = $derived(studio.filter.trim().toLowerCase());
	const match = (...xs: (string | null | undefined)[]) => !q || xs.some((x) => x?.toLowerCase().includes(q));

	const parts = $derived(
		(scene?.parts ?? []).filter((p) => match(p.ref, p.title, p.value, p.kind, p.at, ...p.pins.map((x) => x.hole)))
	);
	const nets = $derived((scene?.nets ?? []).filter((n) => match(n.name, ...n.pins)));
	const wires = $derived(
		(scene?.wires ?? []).filter((w) => match(w.id, w.group, ...w.nets, w.a?.hole, w.b?.hole, w.a?.pin, w.b?.pin))
	);
	const groups = $derived(
		[...new Set(wires.map((w) => (w.flex ? 'flexible jumpers' : w.group)))].map((g) => ({
			name: g,
			wires: wires.filter((w) => (w.flex ? 'flexible jumpers' : w.group) === g)
		}))
	);

	function row(key: Key) {
		return studio.selected === key ? 'bg-primary/10 text-foreground' : studio.hover === key ? 'bg-muted' : '';
	}
</script>

<div class="flex h-full flex-col bg-sidebar">
	{#if scene}
		<div class="px-4 pt-4 pb-3">
			<div class="truncate text-sm font-semibold">{scene.title}</div>
			<div class="mt-0.5 text-xs text-muted-foreground">
				{scene.boards.length === 1 ? `1 ${scene.boards[0].kind}-size board` : `${scene.boards.length} boards`} ·
				{scene.stats.parts} parts · {scene.stats.wires} wires · {scene.stats.nets} nets
			</div>
		</div>
		<div class="px-3 pb-2">
			<div class="relative">
				<Search class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
				<Input bind:value={studio.filter} placeholder="Filter: U1, SDA, f30…" aria-label="Filter parts, nets and wires" class="h-8 pl-8" />
			</div>
		</div>
		<Tabs.Root bind:value={studio.list} class="flex min-h-0 flex-1 flex-col gap-2">
			<Tabs.List class="mx-3 grid grid-cols-3">
				<Tabs.Trigger value="parts">Parts <span class="text-muted-foreground">{parts.length}</span></Tabs.Trigger>
				<Tabs.Trigger value="nets">Nets <span class="text-muted-foreground">{nets.length}</span></Tabs.Trigger>
				<Tabs.Trigger value="wires">Wires <span class="text-muted-foreground">{wires.length}</span></Tabs.Trigger>
			</Tabs.List>
			<ScrollArea class="min-h-0 flex-1">
				<div class="px-2 pb-2">
					{#if studio.list === 'parts'}
						{#each parts as p (p.ref)}
							{@const key = `part:${p.ref}` as Key}
							<button class="flex h-9 w-full items-center gap-2.5 rounded-md px-2 text-left text-sm {row(key)}" {...studio.pointer(key)}>
								<span class="w-12 shrink-0 truncate font-mono text-xs font-semibold">{p.ref}</span>
								<span class="min-w-0 flex-1 truncate text-muted-foreground">{partName(p)}</span>
								<span class="font-mono text-[11px] text-muted-foreground">{p.kind === 'offboard' ? 'off board' : (p.at ?? '—')}</span>
							</button>
						{/each}
					{:else if studio.list === 'nets'}
						{#each nets as n (n.name)}
							{@const key = `net:${n.name}` as Key}
							{@const count = studio.index?.wiresOfNet.get(n.name)?.length ?? 0}
							<button class="flex h-9 w-full items-center gap-2.5 rounded-md px-2 text-left text-sm {row(key)}" {...studio.pointer(key)}>
								<span class="size-2.5 shrink-0 rounded-full ring-1 ring-black/15" style:background={n.color}></span>
								<span class="min-w-0 flex-1 truncate font-mono text-xs font-semibold">{n.name}</span>
								<span class="text-[11px] text-muted-foreground">{n.pins.length} pins · {count} {count === 1 ? 'wire' : 'wires'}</span>
							</button>
						{/each}
					{:else}
						{#each groups as g (g.name)}
							<div class="px-2 pt-2.5 pb-1 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">{g.name} · {g.wires.length}</div>
							{#each g.wires as w (w.id)}
								{@const key = `wire:${w.id}` as Key}
								<button class="flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left text-sm {row(key)}" {...studio.pointer(key)}>
									<span class="h-1 w-4 shrink-0 rounded-full" style:background={w.color}></span>
									<span class="w-9 shrink-0 font-mono text-xs font-semibold">{w.id}</span>
									<span class="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">
										{w.a?.hole ?? w.a?.pin ?? '?'} → {w.b?.hole ?? w.b?.pin ?? '?'}
									</span>
								</button>
							{/each}
						{/each}
					{/if}
				</div>
			</ScrollArea>
		</Tabs.Root>
		<div class="m-3">
			{#if scene.stats.errors > 0}
				<button class="w-full rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-left" onclick={() => (studio.drawer = 'issues')}>
					<div class="flex items-center gap-2 text-sm font-medium text-destructive"><CircleX class="size-4" /> {scene.stats.errors} {scene.stats.errors === 1 ? 'error' : 'errors'}</div>
					<div class="mt-1 text-xs text-muted-foreground">The board doesn't match the nets yet. See Issues below the board.</div>
				</button>
			{:else if scene.stats.warnings > 0}
				<button class="w-full rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-left" onclick={() => (studio.drawer = 'issues')}>
					<div class="flex items-center gap-2 text-sm font-medium text-amber-700 dark:text-amber-400"><TriangleAlert class="size-4" /> Check passed, {scene.stats.warnings} {scene.stats.warnings === 1 ? 'warning' : 'warnings'}</div>
					<div class="mt-1 text-xs text-muted-foreground">It's wired as the nets say; some wires or parts could sit better.</div>
				</button>
			{:else}
				<div class="rounded-lg border border-emerald-600/25 bg-emerald-600/5 p-3">
					<div class="flex items-center gap-2 text-sm font-medium text-emerald-700 dark:text-emerald-400"><CircleCheck class="size-4" /> Wiring check passed</div>
					<div class="mt-1 text-xs text-muted-foreground">What the strips and wires connect matches all {scene.stats.nets} nets.</div>
				</div>
			{/if}
		</div>
	{/if}
</div>
