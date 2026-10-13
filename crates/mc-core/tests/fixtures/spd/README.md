# Independent SPD base-block references

These bytes come from public coreboot references, **not** this decoder or its
test CRC generator. Source revision: `f26c75fcaa17e183651917397c4bd79ccbd55efc`
(retrieved 2026-10-13).

- [`coreboot-ddr3-base.hex`](https://github.com/coreboot/coreboot/blob/f26c75fcaa17e183651917397c4bd79ccbd55efc/util/autoport/readme.md#spd):
  first 128 bytes of the autoport guide's `CH0S0` DDR3 example. Upstream explicitly
  describes it as a reconstructed SPD matching the current controller mode,
  **not a complete EEPROM dump**. It has no serial/vendor/part identity. The
  original short-coverage CRC (`6d 17`) is retained unchanged.
  SHA-256 of the 128 decoded bytes:
  `2c5d23f48a3236b410193ad678da5a1f11967f94af7ff1aee944c891692aaa85`.
- [`coreboot-ddr4-base.hex`](https://github.com/coreboot/coreboot/blob/f26c75fcaa17e183651917397c4bd79ccbd55efc/src/mainboard/intel/harcuvar/spd/micron_4GiB_dimm_MTA9ASF51272PZ-2G1A2.spd.hex):
  first 128 bytes of coreboot's Micron MTA9ASF51272PZ-2G1A2 DDR4 RDIMM
  configuration, git blob `4abad8dd3281b319e1283eb3068d0bd8cd92e9ee`.
  Omit the source comment and all bytes from offset 128 onward; normalize hex
  case/whitespace only. This excludes manufacturing identity/serial fields and
  preserves all base timing bytes and the original CRC (`64 2b`).
  SHA-256 of the 128 decoded bytes:
  `61898bc5b08a414cb6a0d68fead3ecc4d81d429f5edb1423956343a4abb32656`.

Reuse basis: coreboot's [README, “Uncopyrightable files”](https://github.com/coreboot/coreboot/blob/f26c75fcaa17e183651917397c4bd79ccbd55efc/README.md#uncopyrightable-files)
explicitly treats non-creative SPD configuration bytes as public domain,
excluded from its general code license. No upstream prose or executable code
is copied here. These references validate base-block decoding/checksums; they
do not establish live EEPROM access, physical slot placement, or DDR5 mappings.
