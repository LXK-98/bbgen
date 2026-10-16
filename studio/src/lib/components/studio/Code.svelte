<script lang="ts">
	// bbgen's sentences with `holes` in backticks, the backticked parts in mono.
	import { codeSpans } from '#lib/scene.js';

	let { text }: { text: string } = $props();
</script>

{#each codeSpans(text) as span}{#if span.code}<code class="rounded bg-muted px-1 py-px font-mono text-[0.92em]">{span.text}</code>{:else}{span.text}{/if}{/each}
