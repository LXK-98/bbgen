<script lang="ts">
	// Why the newest build has no board: a banner over the last drawing that built, or a card where there's none.
	import { studio } from '#lib/studio.svelte.js';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import ShowInYaml from './ShowInYaml.svelte';

	let { banner = false }: { banner?: boolean } = $props();
	const error = $derived(studio.error);
</script>

{#if error && banner}
	<div class="absolute right-3 bottom-3 left-3 z-10 flex items-start gap-3 rounded-lg border border-destructive/40 bg-background/95 px-4 py-3 text-sm shadow-md">
		<TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
		<div>
			<div class="font-medium">The latest save didn't build; this is the last one that did.</div>
			<div class="mt-0.5 font-mono text-xs text-muted-foreground">
				{error.message}{#if error.line && !error.message.includes('(line')} · line {error.line}{/if}
			</div>
		</div>
		<div class="ml-auto shrink-0"><ShowInYaml line={error.line} variant="ghost" /></div>
	</div>
{:else if error}
	<div class="max-w-lg rounded-xl border border-destructive/40 bg-background p-5 text-sm text-foreground">
		<div class="flex items-center gap-2 font-medium"><TriangleAlert class="size-4 text-destructive" /> bbgen couldn't build this circuit</div>
		<pre class="mt-3 font-mono text-xs whitespace-pre-wrap text-muted-foreground">{error.message}{#if error.line}
line {error.line}{#if error.column}, column {error.column}{/if}{/if}</pre>
		<div class="mt-3"><ShowInYaml line={error.line} /></div>
	</div>
{/if}
