<script lang="ts">
	// What matters about the selected part, wire, net, issue or hole; the circuit's overview when nothing is.
	import { studio } from '#lib/studio.svelte.js';
	import { endName, kindName, partName, wireKind } from '#lib/describe.js';
	import { keyKind, keyName, pinPart, type Key, type Wire } from '#lib/scene.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { ScrollArea } from '#lib/components/ui/scroll-area/index.js';
	import Code from './Code.svelte';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import ArrowRight from '@lucide/svelte/icons/arrow-right';
	import X from '@lucide/svelte/icons/x';
	import Copy from '@lucide/svelte/icons/copy';

	const scene = $derived(studio.scene);
	const ix = $derived(studio.index);
	const sel = $derived(studio.selected && studio.exists(studio.selected) ? studio.selected : null);
	const kind = $derived(keyKind(sel));
	const name = $derived(sel ? keyName(sel) : '');

	const part = $derived(kind === 'part' ? ix?.part.get(name) : undefined);
	const wire = $derived(kind === 'wire' ? ix?.wire.get(name) : undefined);
	const net = $derived(kind === 'net' ? ix?.net.get(name) : undefined);
	const issue = $derived(kind === 'issue' ? scene?.issues[Number(name)] : undefined);

	const step = $derived(sel ? (scene?.checklist.findIndex((s) => s.item === sel) ?? -1) + 1 : 0);
	let allPins = $state(false);

	/** a pin as the scene names it (`U1.5 SDA`) → its hole */
	function pinHole(pin: string): string | null {
		const p = ix?.part.get(pinPart(pin));
		const num = Number(pin.split('.')[1]?.split(' ')[0]);
		return p?.pins.find((x) => x.num === num)?.hole ?? null;
	}

	function inchLength(w: Wire): string {
		let px = 0;
		for (let i = 1; i < w.points.length; i++) px += Math.hypot(w.points[i][0] - w.points[i - 1][0], w.points[i][1] - w.points[i - 1][1]);
		return `${(px / 200).toFixed(1)} in (${Math.round(px / 20)} holes)`;
	}

	function railsOf(netName: string): string[] {
		const out: string[] = [];
		for (const b of scene?.boards ?? [])
			for (const [rail, nets] of Object.entries(b.rails))
				if (nets.includes(netName)) out.push(scene!.boards.length > 1 ? `${b.id}:${rail}` : rail);
		return out;
	}

	const issuesOf = (test: (i: NonNullable<typeof scene>['issues'][number]) => boolean) =>
		(scene?.issues ?? []).map((i, k) => ({ i, k })).filter(({ i }) => test(i));

	const ROT: Record<number, string> = { 0: 'as drawn (rot 0)', 90: 'turned 90°', 180: 'turned round (rot 180)', 270: 'turned 270°' };

	function copy(text: string) {
		navigator.clipboard?.writeText(text);
	}
</script>

{#snippet title(text: string)}
	<h3 class="mb-2 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">{text}</h3>
{/snippet}

{#snippet fact(k: string, v: string | null | undefined, mono = false)}
	{#if v}
		<div class="flex gap-3 py-0.5 text-sm">
			<span class="w-24 shrink-0 text-muted-foreground">{k}</span>
			<span class="min-w-0 {mono ? 'font-mono text-xs leading-5' : ''}">{v}</span>
		</div>
	{/if}
{/snippet}

{#snippet link(key: Key, label: string, color: string | null = null)}
	<button class="inline-flex items-center gap-1.5 rounded px-1 font-mono text-xs text-primary hover:bg-primary/10" {...studio.pointer(key)}>
		{#if color}<span class="size-2 rounded-full ring-1 ring-black/15" style:background={color}></span>{/if}{label}
	</button>
{/snippet}

{#snippet checks(list: { i: { level: string; code: string; message: string }; k: number }[], what: string)}
	<section>
		{@render title('Check')}
		{#if list.length === 0}
			<div class="flex items-center gap-2 text-sm text-emerald-700 dark:text-emerald-400"><CircleCheck class="size-4" /> No issues on {what}</div>
		{:else}
			{#each list as { i, k }}
				<button class="mb-1 block w-full rounded-md border p-2 text-left text-xs hover:bg-muted" {...studio.pointer(`issue:${k}`)}>
					<span class="font-semibold {i.level === 'error' ? 'text-destructive' : 'text-amber-700 dark:text-amber-400'}">{i.code}</span>
					<span class="text-muted-foreground"> {i.message}</span>
				</button>
			{/each}
		{/if}
	</section>
{/snippet}

<div class="flex h-full flex-col">
	<div class="flex h-11 shrink-0 items-center justify-between border-b px-4">
		<span class="text-sm font-semibold">Inspector</span>
		{#if sel}
			<Button variant="ghost" size="icon-sm" aria-label="Clear the selection" onclick={() => studio.select(null)}><X /></Button>
		{/if}
	</div>
	<ScrollArea class="min-h-0 flex-1">
		<div class="space-y-5 p-4">
			{#if part && ix}
				{@const wires = ix.wiresOfPart.get(part.ref) ?? []}
				{@const busy = part.pins.length > 16}
				{@const shown = busy && !allPins ? part.pins.filter((p) => p.net) : part.pins}
				<div class="flex items-start gap-3">
					<div class="min-w-0">
						<div class="flex flex-wrap items-baseline gap-2">
							<h2 class="font-mono text-xl font-semibold">{part.ref}</h2>
							<span class="text-sm text-muted-foreground">{partName(part)}</span>
						</div>
						<div class="mt-1.5 flex flex-wrap gap-1.5">
							<Badge variant="secondary">{kindName(part)}</Badge>
							{#if part.placed_by}<Badge variant="outline">Placed by {part.placed_by}</Badge>{/if}
							{#if part.raised}<Badge variant="outline">Raised on its headers</Badge>{/if}
						</div>
					</div>
				</div>
				<section>
					{@render title('Where it sits')}
					{#if part.kind === 'offboard'}
						{@render fact('Where', 'beside the board, reached by flying leads')}
					{:else if part.legs.length || ['resistor', 'capacitor', 'electrolytic', 'diode', 'led'].includes(part.kind)}
						{@render fact('Legs', part.pins.map((p) => p.hole ?? '?').join(', '), true)}
						{#if part.legs.length}{@render fact('Written as', part.legs.join(', '), true)}{/if}
					{:else}
						{@render fact('Pin 1', part.at, true)}
						{@render fact('Turned', ROT[part.rot] ?? `${part.rot}°`)}
					{/if}
					{@render fact('Value', part.value)}
					{@render fact('From KiCad', part.source, true)}
					{@render fact('Adapter', part.adapter)}
					{@render fact('Keep clear', part.keepouts.join(', '))}
					{@render fact('Slants because', part.slant)}
				</section>
				<section>
					{@render title('How to place it')}
					<div class="rounded-lg border bg-muted/40 p-3 text-sm">
						{#if step}<div class="mb-1 text-xs text-muted-foreground">Checklist step {step} of {scene?.checklist.length}</div>{/if}
						<Code text={part.text} />
					</div>
				</section>
				<section>
					{@render title(`Pins · ${part.pins.length}`)}
					<div class="overflow-hidden rounded-lg border text-xs">
						<div class="grid grid-cols-[2rem_minmax(0,1fr)_4rem_minmax(0,1.2fr)] gap-2 bg-muted/50 px-3 py-1.5 font-semibold text-muted-foreground">
							<span>#</span><span>Name</span><span>Hole</span><span>Net</span>
						</div>
						{#each shown as pin (pin.num)}
							<div class="grid min-h-8 grid-cols-[2rem_minmax(0,1fr)_4rem_minmax(0,1.2fr)] items-center gap-2 border-t px-3 font-mono">
								<span class="text-muted-foreground">{pin.num}</span>
								<span class="truncate">{pin.name}</span>
								<span>{pin.hole ?? '—'}</span>
								<span class="truncate">
									{#if pin.net}{@render link(`net:${pin.net}`, pin.net, ix.net.get(pin.net)?.color)}{:else}<span class="text-muted-foreground">—</span>{/if}
								</span>
							</div>
						{/each}
						{#if busy}
							<button class="w-full border-t px-3 py-2 text-left text-muted-foreground hover:bg-muted" onclick={() => (allPins = !allPins)}>
								{allPins ? 'Show only pins in a net' : `${part.pins.length - shown.length} more pins in no net: show them`}
							</button>
						{/if}
					</div>
				</section>
				{#if wires.length}
					<section>
						{@render title(`Wires · ${wires.length}`)}
						<div class="flex flex-wrap gap-1">
							{#each wires as w}{@render link(`wire:${w}`, w, ix.wire.get(w)?.color)}{/each}
						</div>
					</section>
				{/if}
				{@render checks(issuesOf((i) => i.parts.includes(part.ref)), part.ref)}
				<Button variant="outline" size="sm" onclick={() => copy(part.pins.map((p) => p.hole).filter(Boolean).join(' '))}><Copy /> Copy its holes</Button>
			{:else if wire && ix}
				<div>
					<div class="flex flex-wrap items-center gap-2.5">
						<span class="h-1.5 w-8 rounded-full" style:background={wire.color}></span>
						<h2 class="font-mono text-xl font-semibold">{wire.id}</h2>
						{#each wire.nets as n}{@render link(`net:${n}`, n, ix.net.get(n)?.color)}{/each}
					</div>
					<div class="mt-1.5 flex flex-wrap gap-1.5">
						<Badge variant="secondary">{wire.flex ? 'Flexible jumper' : wire.flying ? 'Flying lead' : 'Rigid wire'}</Badge>
						<Badge variant="outline">Laid by {wire.laid_by}</Badge>
						<Badge variant="outline">{wire.group}</Badge>
					</div>
				</div>
				<div class="grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-2">
					{#each [wire.a, wire.b] as end, k}
						{#if k === 1}<ArrowRight class="size-4 text-muted-foreground" />{/if}
						<div class="rounded-lg border p-2.5">
							<div class="text-[11px] text-muted-foreground">{k === 0 ? 'From' : 'To'}</div>
							<div class="font-mono text-base font-semibold">{endName(end)}</div>
							<div class="truncate text-xs text-muted-foreground">
								{#if end?.pin}{end.pin}{:else if end?.hole}{ix.strip.get(ix.stripOf.get(end.hole) ?? '')?.pins.map((p) => p.pin).join(', ') || (end.hole.includes('+') || end.hole.includes('-') ? 'rail' : 'empty strip')}{/if}
							</div>
						</div>
					{/each}
				</div>
				<section>
					{@render title('Wire')}
					{@render fact('Kind', wireKind(wire))}
					{@render fact('Length', wire.flying || wire.flex ? null : inchLength(wire))}
					{@render fact('Bends', wire.via.join(', '), true)}
					{@render fact('Runs under', wire.under.join(', '))}
					{@render fact('Slants because', wire.slant)}
					{@render fact('Written as', wire.laid_by === 'you' ? `${wire.spec[0]} → ${wire.spec[1]}` : null, true)}
				</section>
				<section>
					{@render title('Checklist line')}
					<div class="rounded-lg border bg-muted/40 p-3 text-sm">
						{#if step}<div class="mb-1 text-xs text-muted-foreground">Step {step} of {scene?.checklist.length}</div>{/if}
						<span class="font-semibold">{wire.id}</span> <Code text={wire.text} />
					</div>
				</section>
				{@render checks(issuesOf((i) => new RegExp(`\\b${wire.id}\\b`).test(i.message)), wire.id)}
			{:else if net && ix}
				{@const wires = ix.wiresOfNet.get(net.name) ?? []}
				{@const rails = railsOf(net.name)}
				<div>
					<div class="flex items-center gap-2.5">
						<span class="size-3.5 rounded-full ring-1 ring-black/15" style:background={net.color}></span>
						<h2 class="font-mono text-xl font-semibold">{net.name}</h2>
					</div>
					<div class="mt-1.5 flex flex-wrap gap-1.5">
						<Badge variant="secondary">{net.polarity === '+' ? 'Supply' : net.polarity === '-' ? 'Ground' : 'Signal'}</Badge>
						{#if net.guessed}<Badge variant="outline">Guessed from its name</Badge>{/if}
					</div>
				</div>
				<section>
					{@render title('Net')}
					{@render fact('Rails', rails.length ? rails.join(', ') : 'none', true)}
					{@render fact('Asked for', net.rails.length ? `rail: ${net.rails.join(', ')}` : null, true)}
				</section>
				<section>
					{@render title(`Pins · ${net.pins.length}`)}
					<div class="overflow-hidden rounded-lg border">
						{#each net.pins as pin}
							<button class="flex h-8 w-full items-center justify-between border-t px-3 font-mono text-xs first:border-t-0 hover:bg-muted" {...studio.pointer(`part:${pinPart(pin)}`)}>
								<span class="font-semibold">{pin}</span><span class="text-muted-foreground">{pinHole(pin) ?? 'off board'}</span>
							</button>
						{/each}
					</div>
				</section>
				<section>
					{@render title(`Wires · ${wires.length}`)}
					{#if wires.length}
						<div class="overflow-hidden rounded-lg border">
							{#each wires as id}
								{@const w = ix.wire.get(id)!}
								<button class="flex h-8 w-full items-center gap-3 border-t px-3 font-mono text-xs first:border-t-0 hover:bg-muted" {...studio.pointer(`wire:${id}`)}>
									<span class="w-9 text-left font-semibold">{id}</span><span class="text-muted-foreground">{endName(w.a)} → {endName(w.b)}</span>
								</button>
							{/each}
						</div>
					{:else}
						<p class="text-sm text-muted-foreground">None: its pins share a strip or a rail.</p>
					{/if}
				</section>
				{@render checks(issuesOf((i) => new RegExp(`\\b${net.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\b`).test(i.message)), net.name)}
			{:else if issue && ix}
				<div>
					<Badge variant={issue.level === 'error' ? 'destructive' : 'secondary'}>{issue.level}</Badge>
					<h2 class="mt-2 font-mono text-xl font-semibold">{issue.code}</h2>
					<p class="mt-2 text-sm">{issue.message}</p>
				</div>
				{#if issue.parts.length}
					<section>{@render title('Parts')}<div class="flex flex-wrap gap-1">{#each issue.parts as r}{@render link(`part:${r}`, r)}{/each}</div></section>
				{/if}
				{#if issue.pins.length}<section>{@render title('Pins')}<p class="font-mono text-xs">{issue.pins.join(', ')}</p></section>{/if}
				{#if issue.holes.length}<section>{@render title('Holes')}<div class="flex flex-wrap gap-1">{#each issue.holes as h}{@render link(`hole:${h}`, h)}{/each}</div></section>{/if}
			{:else if kind === 'hole' && ix}
				{@const s = ix.strip.get(ix.stripOf.get(name) ?? '')}
				<div>
					<h2 class="font-mono text-xl font-semibold">{name}</h2>
					<p class="mt-1 text-sm text-muted-foreground">Strip {s?.label}: {s?.holes.length} holes joined under the board.</p>
				</div>
				<section>
					{@render title('In this hole')}
					<p class="font-mono text-xs">{(ix.inHole.get(name) ?? []).join(', ') || 'nothing'}</p>
				</section>
				<section>
					{@render title('In its strip')}
					{@render fact('Net', s?.nets.join(' / ') || 'none', true)}
					{#if s?.pins.length}<div class="mt-1 flex flex-wrap gap-1">{#each s.pins as p}{@render link(`part:${p.part}`, p.pin)}{/each}</div>{/if}
					{#if s?.wires.length}<div class="mt-1 flex flex-wrap gap-1">{#each s.wires as w}{@render link(`wire:${w}`, w, ix.wire.get(w)?.color)}{/each}</div>{/if}
				</section>
			{:else if scene}
				<div>
					<h2 class="text-base font-semibold">{scene.title}</h2>
					<p class="mt-1 text-sm text-muted-foreground">Hover anything on the board, or in the lists, to light it up. Click it to see it here.</p>
				</div>
				{#if scene.notes.length}
					<section>
						{@render title('Notes')}
						{#each scene.notes as note}<p class="mb-1 rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-sm">{note}</p>{/each}
					</section>
				{/if}
				<section>
					{@render title(scene.boards.length === 1 ? 'Board' : 'Boards')}
					{#each scene.boards as b}
						<div class="mb-2 text-sm">
							<div class="mb-1">{scene.boards.length > 1 ? `Board ${b.id}: ` : ''}{b.kind}-size, {b.columns} columns{b.split_rails ? ', rails split' : ''}</div>
							{#each Object.entries(b.rails) as [rail, nets]}
								<div class="flex gap-3 font-mono text-xs"><span class="w-12 text-muted-foreground">{rail}</span><span>{nets.length ? nets.join(' / ') : 'unused'}</span></div>
							{/each}
						</div>
					{/each}
				</section>
				<section>
					{@render title('Build')}
					{@render fact('Parts', String(scene.stats.parts))}
					{@render fact('Wires', `${scene.stats.wires}, ${scene.wires.filter((w) => w.laid_by === 'bbgen').length} laid by bbgen`)}
					{@render fact('Placed by bbgen', scene.parts.filter((p) => p.placed_by === 'bbgen').map((p) => p.ref).join(', ') || 'none')}
					{@render fact('Took', `${scene.stats.ms} ms`)}
				</section>
			{/if}
		</div>
	</ScrollArea>
</div>
