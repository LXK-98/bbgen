# ELK's layouts, as recorded

`tests/elk.rs` lays out each `*.in.json` with bbgen's ELK (elk-rs, [src/elk.rs](../../src/elk.rs)) and holds it to
`*.out.json`. These were elkjs's layouts in Node until bbgen moved to elk-rs (October 2026); elk-rs's were the same
to 1e-14 px, and replaced them.

- `eeprom` and `register`: `examples/eeprom.yaml` and benchmark 13 as the spike drew them
  ([docs/design/circuit-view-plan.md](../../docs/design/circuit-view-plan.md)): each chip a box with its pins in DIP order, each
  signal net edges from its first pin.
- `ports`: ports on every side, fixed in place or only to a side, edges from one port merged, a node on its own.

After upgrading elk-rs, look at the schematics (`tests/schematic.rs` holds them to their records), then record
these again:

```bash
BBGEN_RECORD=1 cargo test --release --test elk
```
