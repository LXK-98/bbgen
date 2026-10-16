<script lang="ts">
	import { onMount } from 'svelte';
	import { studio } from '#lib/studio.svelte.js';
	import * as Resizable from '#lib/components/ui/resizable/index.js';
	import * as Tooltip from '#lib/components/ui/tooltip/index.js';
	import TopBar from '#lib/components/studio/TopBar.svelte';
	import Sidebar from '#lib/components/studio/Sidebar.svelte';
	import Board from '#lib/components/studio/Board.svelte';
	import Circuit from '#lib/components/studio/Circuit.svelte';
	import Generation from '#lib/components/studio/Generation.svelte';
	import Progress from '#lib/components/studio/Progress.svelte';
	import Drawer from '#lib/components/studio/Drawer.svelte';
	import Inspector from '#lib/components/studio/Inspector.svelte';
	import StatusBar from '#lib/components/studio/StatusBar.svelte';
	import OpenDialog from '#lib/components/studio/OpenDialog.svelte';
	import DropZone from '#lib/components/studio/DropZone.svelte';

	onMount(() => {
		studio.init();
	});

	function onkeydown(e: KeyboardEvent) {
		if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'o') {
			e.preventDefault();
			studio.openDialog = true;
		} else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'r') {
			e.preventDefault();
			studio.rebuild();
		} else if (e.key === 'Escape' && !(e.target as HTMLElement).closest('input, textarea')) {
			studio.select(null);
		}
	}
</script>

<svelte:window {onkeydown} />
<svelte:head><title>{studio.scene ? `${studio.scene.title} · bbgen studio` : 'bbgen studio'}</title></svelte:head>

<Tooltip.Provider delayDuration={300}>
	<div class="flex h-screen flex-col overflow-hidden bg-background text-foreground">
		<TopBar />
		<Resizable.PaneGroup direction="horizontal" class="min-h-0 flex-1" autoSaveId="bbgen-studio.columns">
			<Resizable.Pane defaultSize={19} minSize={14} maxSize={32}>
				<Sidebar />
			</Resizable.Pane>
			<Resizable.Handle />
			<Resizable.Pane defaultSize={57} minSize={30}>
				<Resizable.PaneGroup direction="vertical" autoSaveId="bbgen-studio.rows">
					<Resizable.Pane defaultSize={72} minSize={30} class="relative">
						{#if studio.mode === 'generation'}
							<Generation />
						{:else if studio.view === 'both'}
							<Resizable.PaneGroup direction="horizontal" autoSaveId="bbgen-studio.both">
								<Resizable.Pane defaultSize={50} minSize={20}><Board /></Resizable.Pane>
								<Resizable.Handle />
								<Resizable.Pane defaultSize={50} minSize={20}><Circuit /></Resizable.Pane>
							</Resizable.PaneGroup>
						{:else if studio.view === 'circuit'}
							<Circuit />
						{:else}
							<Board />
						{/if}
						<DropZone />
					</Resizable.Pane>
					<Resizable.Handle />
					<Resizable.Pane defaultSize={28} minSize={10}>
						{#if studio.mode === 'generation'}<Progress />{:else}<Drawer />{/if}
					</Resizable.Pane>
				</Resizable.PaneGroup>
			</Resizable.Pane>
			<Resizable.Handle />
			<Resizable.Pane defaultSize={24} minSize={16} maxSize={40}>
				<Inspector />
			</Resizable.Pane>
		</Resizable.PaneGroup>
		<StatusBar />
	</div>
	<OpenDialog />
</Tooltip.Provider>
