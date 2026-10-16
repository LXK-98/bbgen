<script lang="ts">
	// Light, dark, or follow the system (the default). mode-watcher remembers the choice.
	import { mode, resetMode, setMode, userPrefersMode } from 'mode-watcher';
	import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
	import { buttonVariants } from '#lib/components/ui/button/index.js';
	import Sun from '@lucide/svelte/icons/sun';
	import Moon from '@lucide/svelte/icons/moon';
	import Monitor from '@lucide/svelte/icons/monitor';

	const choice = $derived(userPrefersMode.current);
	function pick(value: string) {
		if (value === 'system') resetMode();
		else setMode(value as 'light' | 'dark');
	}
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class={buttonVariants({ variant: 'ghost', size: 'icon-sm' })} aria-label="Theme">
		{#if choice === 'system'}<Monitor />{:else if mode.current === 'dark'}<Moon />{:else}<Sun />{/if}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end" class="w-36">
		<DropdownMenu.RadioGroup value={choice} onValueChange={pick}>
			<DropdownMenu.RadioItem value="light"><Sun /> Light</DropdownMenu.RadioItem>
			<DropdownMenu.RadioItem value="dark"><Moon /> Dark</DropdownMenu.RadioItem>
			<DropdownMenu.RadioItem value="system"><Monitor /> System</DropdownMenu.RadioItem>
		</DropdownMenu.RadioGroup>
	</DropdownMenu.Content>
</DropdownMenu.Root>
