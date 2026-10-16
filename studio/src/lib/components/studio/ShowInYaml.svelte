<script lang="ts">
	// Opens the circuit file in an editor at the line something is written on.
	import { studio } from '#lib/studio.svelte.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import FileCode from '@lucide/svelte/icons/file-code';

	let { line, variant = 'outline' }: { line: number | null; variant?: 'outline' | 'ghost' } = $props();
</script>

{#if line}
	<Button {variant} size="sm" onclick={() => studio.edit(line)}><FileCode /> Show in YAML · line {line}</Button>
{/if}
