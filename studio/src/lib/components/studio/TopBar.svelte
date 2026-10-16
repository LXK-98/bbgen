<script lang="ts">
	// The file, the mode (only Inspect so far), the check's result and Rebuild.
	import { studio } from '#lib/studio.svelte.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Kbd } from '#lib/components/ui/kbd/index.js';
	import * as Tooltip from '#lib/components/ui/tooltip/index.js';
	import FileCode from '@lucide/svelte/icons/file-code';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import Eye from '@lucide/svelte/icons/eye';
	import ThemeMenu from './ThemeMenu.svelte';

	const scene = $derived(studio.scene);
	const file = $derived(studio.path?.split('/').pop());
	const dir = $derived(studio.path?.split('/').slice(-2, -1)[0]);
	const MODES = [
		{ name: 'Inspect', ready: true },
		{ name: 'Build guide', ready: false },
		{ name: 'Generation', ready: false },
		{ name: 'Edit', ready: false }
	];
</script>

<header class="flex h-12 shrink-0 items-center gap-4 border-b bg-background px-3">
	<div class="flex items-center gap-2 pl-1">
		<svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.7">
			<rect x="2.5" y="4.5" width="19" height="15" rx="2.5" />
			<line x1="2.5" y1="12" x2="21.5" y2="12" stroke-dasharray="1.5 2" />
			<path d="M7 8.2 C 7 13, 15 11, 15 15.8" class="stroke-primary" />
		</svg>
		<span class="text-sm font-semibold">bbgen studio</span>
	</div>

	<Button variant="outline" size="sm" onclick={() => (studio.openDialog = true)} class="max-w-80">
		<FileCode />
		{#if file}<span class="truncate font-mono text-xs"><span class="text-muted-foreground">{dir}/</span>{file}</span>{:else}Open a circuit{/if}
		<Kbd class="ml-1">Ctrl O</Kbd>
	</Button>

	<nav aria-label="Mode" class="flex items-center gap-0.5 rounded-lg bg-muted p-0.5">
		{#each MODES as m}
			{#if m.ready}
				<span aria-current="page" class="flex h-7 items-center gap-1.5 rounded-md bg-background px-3 text-xs font-medium shadow-sm"><Eye class="size-3.5" /> {m.name}</span>
			{:else}
				<Tooltip.Root>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<span {...props} class="flex h-7 items-center px-3 text-xs text-muted-foreground/70">{m.name}</span>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Content>Planned: see docs/studio-plan.md</Tooltip.Content>
				</Tooltip.Root>
			{/if}
		{/each}
	</nav>

	<div class="ml-auto flex items-center gap-3">
		{#if studio.watching && studio.builtAt}
			<span class="hidden items-center gap-1.5 text-xs text-muted-foreground lg:flex">
				<span class="size-1.5 rounded-full bg-emerald-500"></span>
				Rebuilds on save · built {studio.builtAt.toLocaleTimeString()}
			</span>
		{/if}
		{#if studio.error}
			<Badge variant="destructive"><CircleX /> Didn't build</Badge>
		{:else if scene}
			{#if scene.stats.errors}
				<Badge variant="destructive"><CircleX /> {scene.stats.errors} {scene.stats.errors === 1 ? 'error' : 'errors'}</Badge>
			{:else if scene.stats.warnings}
				<Badge variant="outline" class="text-amber-700 dark:text-amber-400"><TriangleAlert /> {scene.stats.warnings} {scene.stats.warnings === 1 ? 'warning' : 'warnings'}</Badge>
			{:else}
				<Badge variant="outline" class="text-emerald-700 dark:text-emerald-400"><CircleCheck /> Check passed</Badge>
			{/if}
		{/if}
		<Button size="sm" disabled={!studio.path || studio.building} onclick={() => studio.rebuild()}>
			<RefreshCw class={studio.building ? 'animate-spin' : ''} /> Rebuild
		</Button>
		<ThemeMenu />
	</div>
</header>
