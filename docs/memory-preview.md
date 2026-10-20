# Memory accounting and the Memory launch preset

This is the normal application's RAM accounting, not an opt-in formula or a
temporary experiment. `--memory-preview` is only a convenient launch preset:

```sh
cargo run -p taskmanager-app -- --memory-preview --synthetic 300
```

It opens Performance / Memory from defaults, without reading, migrating or writing
saved GUI settings, claiming the normal desktop instance or installing shortcuts.
Without `--synthetic` or `--fixture`, it still uses the live magpie backend; it is
not a backend sandbox. Explicit `--dump-readings` and `--stats-file` options retain
their normal behavior. Ordinary launches continue to restore saved settings.

The sidebar, RAM graph, primary Memory statistic and Processes header all use
`MemTotal - min(MemFree, MemTotal)`: occupied physical RAM **including cache**.
`Free` is the complementary number. `Of used: cache` is a component, not something
to add to occupied RAM. It matches the composition segment: `Cached - Shmem +
SReclaimable`, saturated and limited to remaining occupied memory. The other
composition segments are Other, Shared and Buff; they and Cache sum to occupied.

MemAvailable remains in the metadata popup as an estimate. It is not used for the
main occupancy percentage. Commit and compression are separate, overlapping views,
not extra physical allocations. Free no longer includes the collector's ZFS ARC
adjustment; ARC remains occupied, while the existing availability adjustment stays.

When every slot has known capacity and the slot inventory is complete, the title
shows installed capacity and reported memory kind, e.g. `32 GiB · DIMM DDR4` or
`16 GiB · SO-DIMM DDR5`. LPDDR retains the reported technology (including an X
suffix only if actually reported), without inventing a DIMM package. Empty slots
are ignored; different populated kinds are listed separately. Hovering shows installed and OS-managed
capacity, their difference and its explanation; the difference is also in metadata as
Outside OS-managed RAM. This includes firmware exclusions **and** boot/kernel
allocations and must not be labelled entirely Hardware reserved. Unknown/incomplete
inventory does not invent an installed total. A complete inventory has exactly the
expected number of slots and unique in-range indices (or no indices at all for
legacy fixtures); mixed indexed/unindexed records are not authoritative. Explicitly
empty slots contribute zero; unknown size or arithmetic overflow prevents a total.
The installed total must be positive and at least the OS-managed capacity.
Capacity accounting does not perform
privileged reads or add work to the UI/first collection.

Own design: existing Win10 geometry, typography and colours are unchanged. Statistic
labels change to Used (incl. cache), Free, Of used: cache, Committed. Composition
caption becomes Physical memory composition, and its residual segment is Other,
not a second competing Used figure. This is intentionally not Windows' primary
usage definition; it presents physical occupancy.

Device identities appear in every Performance sidebar row: type,
model, current readings in the existing three-line layout. Long values are elided
and available in full on hover. RAM uses the common populated-module part number
(with manufacturer if consistent); mixed parts show Mixed modules, missing identity
shows Unknown. Empty slots and firmware placeholders are not treated as models.
Only explicitly empty slots are ignored: unknown occupancy or an incomplete slot
inventory cannot establish a common model. Identity and capacity refresh as late
inventory arrives. The existing sizing rule is preserved: two text lines use a
60 px row / 40 px graph, three lines a 66 px row / 46 px graph. CPU, RAM and battery
groups now normally use three lines; fonts and colours are unchanged.

Ship the GUI with the matching backend: its MemFree stays raw on ZFS too. ARC is
occupied RAM, not an invented cache segment; the protocol has no separate ARC field.
