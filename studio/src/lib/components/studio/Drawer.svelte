<script lang="ts">
	// Under the board: the build checklist, the check's issues, and what bbgen said.
	import { studio } from '#lib/studio.svelte.js';
	import { SECTION_WORD, type Key } from '#lib/scene.js';
	import * as Tabs from '#lib/components/ui/tabs/index.js';
	import { ScrollArea } from '#lib/components/ui/scroll-area/index.js';
	import Code from './Code.svelte';
	import BbgenSetting from './BbgenSetting.svelte';
	import CircleCheck from '@lucide/svelte/icons/circle-check';

	const scene = $derived(studio.scene);

	function row(key: Key) {
		return studio.selected === key ? 'bg-primary/10' : studio.hover === key ? 'bg-muted' : '';
	}
</script>

<Tabs.Root bind:value={studio.drawer} class="flex h-full flex-col gap-0 bg-background">
	<div class="flex h-10 shrink-0 items-center border-b px-2">
		<Tabs.List variant="line">
			<Tabs.Trigger value="checklist">Checklist <span class="text-muted-foreground">{scene?.checklist.length ?? 0}</span></Tabs.Trigger>
			<Tabs.Trigger value="issues">
				Issues
				<span class={scene?.stats.errors ? 'text-destructive' : 'text-muted-foreground'}>{scene?.issues.length ?? 0}</span>
			</Tabs.Trigger>
			<Tabs.Trigger value="output">bbgen output</Tabs.Trigger>
		</Tabs.List>
	</div>
	<ScrollArea class="min-h-0 flex-1">
		<div class="p-1.5">
			{#if studio.drawer === 'checklist'}
				{#each scene?.checklist ?? [] as s, k}
					{@const key = s.item as Key}
					<button class="flex w-full items-baseline gap-3 rounded-md px-2.5 py-1.5 text-left text-sm {row(key)}" {...studio.pointer(key)}>
						<span class="w-6 shrink-0 text-right font-mono text-[11px] text-muted-foreground">{k + 1}</span>
						<span class="w-16 shrink-0 truncate text-[11px] text-muted-foreground">{SECTION_WORD[s.section] ?? s.section}</span>
						<span class="w-11 shrink-0 font-mono text-xs font-semibold">{s.item.split(':')[1]}</span>
						<span class="min-w-0 flex-1"><Code text={s.text} /></span>
					</button>
				{/each}
			{:else if studio.drawer === 'issues'}
				{#if scene && scene.issues.length === 0}
					<div class="flex items-start gap-2 p-3 text-sm">
						<CircleCheck class="mt-0.5 size-4 text-emerald-600" />
						<div>
							<div class="font-medium">No issues</div>
							<div class="text-muted-foreground">No opens, shorts or strays; nothing slanted, covered or in the trench.</div>
						</div>
					</div>
				{/if}
				{#each scene?.issues ?? [] as i, k}
					{@const key = `issue:${k}` as Key}
					<button class="flex w-full items-baseline gap-3 rounded-md px-2.5 py-1.5 text-left text-sm {row(key)}" {...studio.pointer(key)}>
						<span class="w-14 shrink-0 text-xs font-medium {i.level === 'error' ? 'text-destructive' : 'text-amber-700 dark:text-amber-400'}">{i.level}</span>
						<span class="w-28 shrink-0 font-mono text-xs font-semibold">{i.code}</span>
						<span class="min-w-0 flex-1">{i.message}</span>
					</button>
				{/each}
			{:else}
				<BbgenSetting />
			{/if}
			{#if studio.drawer === 'output' && studio.output}
				<pre class="p-2 font-mono text-xs leading-5 whitespace-pre-wrap">$ {studio.output.command}
{#if studio.output.stderr.trim()}{studio.output.stderr.trim()}
{/if}exit {studio.output.code ?? '—'} · {studio.output.ms} ms{#if scene} · scene {scene.schema}, bbgen {scene.bbgen}{/if}</pre>
			{/if}
		</div>
	</ScrollArea>
</Tabs.Root>
