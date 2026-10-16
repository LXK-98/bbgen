<script lang="ts">
	// Which bbgen the studio runs, its version and how it was found; and a way to pick another.
	import { studio } from '#lib/studio.svelte.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import FolderOpen from '@lucide/svelte/icons/folder-open';

	let editing = $state(false);
	let path = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);
	const info = $derived(studio.bbgen);

	async function use(p: string | null) {
		busy = true;
		error = await studio.useBbgen(p);
		busy = false;
		if (!error) editing = false;
	}

	async function browse() {
		const p = await studio.engine?.pickBbgen?.();
		if (p) {
			path = p;
			use(p);
		}
	}
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-md border bg-muted/40 px-3 py-2 text-xs">
	{#if info}
		<span class="font-medium {info.error ? 'text-destructive' : ''}">{info.version ?? "bbgen doesn't run"}</span>
		<span class="min-w-0 truncate font-mono text-muted-foreground" title={info.path}>{info.path}</span>
		<span class="text-muted-foreground">({info.how})</span>
	{:else}
		<span class="text-muted-foreground">Looking for bbgen…</span>
	{/if}
	<span class="ml-auto flex gap-1">
		{#if info?.how === 'picked in the studio'}
			<Button variant="ghost" size="sm" disabled={busy} onclick={() => use(null)}>Use the usual one</Button>
		{/if}
		<Button variant="ghost" size="sm" onclick={() => ((editing = !editing), (path = info?.path ?? ''), (error = null))}>
			{editing ? 'Cancel' : 'Change…'}
		</Button>
	</span>
	{#if info?.error && !editing}<div class="w-full text-destructive">{info.error}</div>{/if}
	{#if editing}
		<form class="flex w-full gap-2" onsubmit={(e) => (e.preventDefault(), use(path.trim() || null))}>
			<Input bind:value={path} placeholder="Full path of a bbgen binary (empty: the usual one)" class="h-8 font-mono text-xs" />
			{#if studio.engine?.pickBbgen}
				<Button type="button" variant="outline" size="sm" disabled={busy} onclick={browse}><FolderOpen /> Browse…</Button>
			{/if}
			<Button type="submit" size="sm" disabled={busy}>Use it</Button>
		</form>
		{#if error}<div class="w-full text-destructive">{error}</div>{/if}
		<div class="w-full text-muted-foreground">
			Without one picked here: <span class="font-mono">$BBGEN</span>, else a bbgen beside the studio, else this
			checkout's <span class="font-mono">bin/bbgen</span>, else <span class="font-mono">bbgen</span> on PATH.
		</div>
	{/if}
</div>
