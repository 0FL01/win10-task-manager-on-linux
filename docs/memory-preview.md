# Memory accounting preview (local experiment)

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
inventory does not invent an installed total. Capacity accounting does not perform
privileged reads or add work to the UI/first collection.

Own design: existing Win10 geometry, typography and colours are unchanged. Statistic
labels change to Used (incl. cache), Free, Of used: cache, Committed. Composition
caption becomes Physical memory composition, and its residual segment is Other,
not a second competing Used figure. This is intentionally not Windows' primary
usage definition; this preview evaluates physical occupancy presentation.

The preview also adds device identities to every Performance sidebar row: type,
model, current readings in the existing three-line layout. Long values are elided
and available in full on hover. RAM uses the common populated-module part number
(with manufacturer if consistent); mixed parts show Mixed modules, missing identity
shows Unknown. Empty slots and firmware placeholders are not treated as models.
