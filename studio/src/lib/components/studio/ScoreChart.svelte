<script lang="ts">
	// The score of every layout bbgen tried, as it tried them: a dot for each (kept ones larger, in the accent), the
	// best so far as a line, the phases as bands, the wires laid as ticks at the foot. Lower is better. The scale is
	// logarithmic: an error costs about 4000, so a move that clears one drops the score from thousands to hundreds.
	// Clicking picks the step under the pointer.
	import type { StepEvent } from '#lib/scene.js';

	interface Props {
		steps: StepEvent[];
		/** the step shown, marked with a line */
		shown: number | null;
		onpick: (n: number) => void;
		class?: string;
	}

	let { steps, shown, onpick, class: klass = '' }: Props = $props();

	let w = $state(0);
	let h = $state(0);
	const PAD = { l: 44, r: 10, t: 18, b: 14 };
	const LABELS = { start: 'starting layouts', turn: 'turning', repair: 'moving parts', wiring: 'wiring' };

	const scored = $derived(steps.filter((e) => e.score !== null));
	const last = $derived(steps.length ? steps[steps.length - 1].n : 1);
	const f = (s: number) => Math.log10(Math.max(s, 0) + 1);
	const range = $derived.by(() => {
		const all = scored.flatMap((e) => (e.from === null ? [e.score!] : [e.score!, e.from]));
		return all.length ? { lo: Math.min(...all), hi: Math.max(...all) } : { lo: 0, hi: 1 };
	});
	const x = (n: number) => PAD.l + ((n - 1) / Math.max(last - 1, 1)) * Math.max(w - PAD.l - PAD.r, 1);
	const y = (s: number) => {
		const span = f(range.hi) - f(range.lo) || 1;
		return PAD.t + ((f(range.hi) - f(s)) / span) * Math.max(h - PAD.t - PAD.b, 1);
	};

	/** the phases as runs of steps */
	const bands = $derived.by(() => {
		const out: { phase: StepEvent['phase']; n0: number; n1: number }[] = [];
		for (const e of steps) {
			const b = out[out.length - 1];
			if (b && b.phase === e.phase) b.n1 = e.n;
			else out.push({ phase: e.phase, n0: e.n, n1: e.n });
		}
		return out;
	});

	/** the best so far: down at each kept step, flat in between */
	const best = $derived.by(() => {
		const kept = scored.filter((e) => e.kept);
		if (!kept.length) return '';
		let d = `M${x(kept[0].n)},${y(kept[0].score!)}`;
		for (const e of kept.slice(1)) d += `H${x(e.n)}V${y(e.score!)}`;
		return d + `H${x(scored[scored.length - 1].n)}`;
	});

	/** labels on the score axis: the lowest and highest, and the powers of ten between */
	const ticks = $derived.by(() => {
		const out = [range.lo, range.hi];
		for (let p = 10; p < range.hi; p *= 10) if (p > range.lo * 1.6 && p < range.hi / 1.6) out.push(p);
		return [...new Set(out.map((v) => Math.round(v)))];
	});

	function pick(e: MouseEvent) {
		const box = (e.currentTarget as SVGElement).getBoundingClientRect();
		const n = Math.round(1 + ((e.clientX - box.left - PAD.l) / Math.max(w - PAD.l - PAD.r, 1)) * Math.max(last - 1, 1));
		onpick(Math.max(1, Math.min(last, n)));
	}
</script>

<div class="relative min-w-0 {klass}" bind:clientWidth={w} bind:clientHeight={h}>
	{#if !scored.length}
		<div class="flex h-full items-center justify-center px-6 text-center text-xs text-muted-foreground">
			{steps.length ? 'Every part was placed in the file: bbgen only wired it, so there are no scores.' : 'The score of each layout bbgen tries shows here.'}
		</div>
	{/if}
	{#if steps.length && w && h}
		<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
		<svg width={w} height={h} class="absolute inset-0 cursor-crosshair select-none" onclick={pick}
			role="img" aria-label="The score of each layout tried, lower is better">
			{#each bands as b, i}
				<rect x={x(b.n0 - 0.5)} y={0} width={Math.max(x(b.n1 + 0.5) - x(b.n0 - 0.5), 1)} height={h}
					class={i % 2 ? 'fill-muted/60' : 'fill-transparent'} />
				{#if x(b.n1) - x(b.n0) > 40 || bands.length === 1}
					<text x={x(b.n0 - 0.5) + 4} y={11} class="fill-muted-foreground text-[10px]">{LABELS[b.phase]}</text>
				{/if}
			{/each}
			{#if scored.length}
				{#each ticks as t}
					<line x1={PAD.l} x2={w - PAD.r} y1={y(t)} y2={y(t)} class="stroke-border" stroke-dasharray="2 3" />
					<text x={PAD.l - 6} y={y(t) + 3} text-anchor="end" class="fill-muted-foreground font-mono text-[10px]">{t}</text>
				{/each}
				{#each scored as e (e.n)}
					<circle cx={x(e.n)} cy={y(e.score!)} r={e.kept ? 3.2 : 2}
						class={e.kept ? 'fill-primary' : 'fill-muted-foreground/45'}>
						<title>{e.n}. {e.caption}: {e.from ?? '–'} → {e.score} {e.kept ? '✓ kept' : '✗'}</title>
					</circle>
				{/each}
				<path d={best} fill="none" class="stroke-primary" stroke-width="1.5" />
			{/if}
			{#each steps.filter((e) => e.phase === 'wiring') as e (e.n)}
				<line x1={x(e.n)} x2={x(e.n)} y1={h - PAD.b + 3} y2={h - 2} class="stroke-primary" stroke-width="1.5">
					<title>{e.n}. {e.caption}</title>
				</line>
			{/each}
			{#if shown !== null}
				<line x1={x(shown)} x2={x(shown)} y1={0} y2={h} class="stroke-foreground/60" stroke-dasharray="3 3" />
			{/if}
		</svg>
	{/if}
</div>
