# ELK's layouts, as Node gives them

`tests/elk.rs` lays out each `*.in.json` with bbgen's ELK (elkjs in QuickJS, [src/elk.rs](../../src/elk.rs)) and
holds it to `*.out.json`: the same graph laid out by the vendored elkjs in Node.

- `eeprom` and `register`: `examples/eeprom.yaml` and benchmark 13 as the spike drew them
  ([docs/circuit-view-plan.md](../../docs/circuit-view-plan.md)): each chip a box with its pins in DIP order, each
  signal net edges from its first pin.
- `ports`: ports on every side, fixed in place or only to a side, edges from one port merged, a node on its own.

After updating `vendor/elkjs`, record them again (needs Node) and see that the tests pass:

```bash
node tests/elk/record.mjs
```
