<script lang="ts">
	// Open a circuit: a recent one, one of bbgen's examples or benchmarks, or any file by its path.
	import { studio } from '#lib/studio.svelte.js';
	import * as Command from '#lib/components/ui/command/index.js';
	import FileCode from '@lucide/svelte/icons/file-code';
	import History from '@lucide/svelte/icons/history';
	import FolderOpen from '@lucide/svelte/icons/folder-open';

	let search = $state('');
	const looksLikePath = $derived(/^(\/|~|[A-Za-z]:\\).*\.ya?ml$/i.test(search.trim()));
	const short = (p: string) => p.split('/').slice(-2).join('/');

	function choose(path: string) {
		studio.openDialog = false;
		search = '';
		studio.open(path);
	}

	async function browse() {
		const path = await studio.engine?.pick?.();
		if (path) choose(path);
	}
</script>

<Command.Dialog bind:open={studio.openDialog} title="Open a circuit" description="Pick a circuit file to build">
	<Command.Input bind:value={search} placeholder="Search, or type the full path of a .yaml file…" />
	<Command.List>
		<Command.Empty>{looksLikePath ? 'Press Enter to open it.' : 'Nothing matches.'}</Command.Empty>
		{#if looksLikePath}
			<Command.Group heading="Path">
				<Command.Item value={search} onSelect={() => choose(search.trim())}><FileCode /> {search.trim()}</Command.Item>
			</Command.Group>
		{/if}
		{#if studio.engine?.pick}
			<Command.Group heading="File">
				<Command.Item value="browse for a file" onSelect={browse}><FolderOpen /> Browse for a file…</Command.Item>
			</Command.Group>
		{/if}
		{#if studio.recent.length}
			<Command.Group heading="Recent">
				{#each studio.recent as p (p)}
					<Command.Item value={`recent ${p}`} onSelect={() => choose(p)}>
						<History /> <span class="font-mono text-xs">{short(p)}</span>
					</Command.Item>
				{/each}
			</Command.Group>
		{/if}
		{#each [['examples', 'Examples'], ['benchmarks', 'Benchmarks']] as [dir, heading]}
			{@const files = studio.examples.filter((p) => p.includes(`/${dir}/`))}
			{#if files.length}
				<Command.Group {heading}>
					{#each files as p (p)}
						<Command.Item value={p} onSelect={() => choose(p)}>
							<FileCode /> <span class="font-mono text-xs">{short(p)}</span>
						</Command.Item>
					{/each}
				</Command.Group>
			{/if}
		{/each}
	</Command.List>
</Command.Dialog>
