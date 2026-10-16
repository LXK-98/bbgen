<script lang="ts">
	// bbgen's files for the open circuit, written where you choose: the board as `bbgen build` draws it, the bare
	// board, the checklist, the schematic, or all of them into a folder.
	import { studio } from '#lib/studio.svelte.js';
	import type { Export } from '#lib/engine.js';
	import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
	import { buttonVariants } from '#lib/components/ui/button/index.js';
	import Download from '@lucide/svelte/icons/download';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Frame from '@lucide/svelte/icons/frame';
	import SquareDashed from '@lucide/svelte/icons/square-dashed';
	import ListChecks from '@lucide/svelte/icons/list-checks';
	import Workflow from '@lucide/svelte/icons/workflow';
	import FolderDown from '@lucide/svelte/icons/folder-down';

	const ITEMS = [
		{ what: 'board', label: 'Board', hint: 'with its legend · .svg', icon: Frame },
		{ what: 'bare', label: 'Bare board', hint: 'the board alone · .svg', icon: SquareDashed },
		{ what: 'checklist', label: 'Checklist', hint: '.md', icon: ListChecks },
		{ what: 'schematic', label: 'Schematic', hint: '.svg', icon: Workflow }
	] as const;

	const folder = $derived(studio.engine?.kind === 'tauri');
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class={buttonVariants({ variant: 'outline', size: 'sm' })} disabled={!studio.path || studio.exporting}>
		{#if studio.exporting}<LoaderCircle class="animate-spin" />{:else}<Download />{/if} Export
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end" class="w-80">
		{#each ITEMS as item}
			<DropdownMenu.Item onSelect={() => studio.export(item.what as Export)}>
				<item.icon /> {item.label}
				<span class="ml-auto pl-4 text-xs whitespace-nowrap text-muted-foreground">
					{item.what === 'schematic' && studio.wires ? 'no labels · .svg' : item.hint}
				</span>
			</DropdownMenu.Item>
		{/each}
		<DropdownMenu.Separator />
		<DropdownMenu.Item onSelect={() => studio.export('all')}>
			<FolderDown /> {folder ? 'All three…' : 'All three'}
			<span class="ml-auto pl-4 text-xs whitespace-nowrap text-muted-foreground">board, checklist, schematic</span>
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>
