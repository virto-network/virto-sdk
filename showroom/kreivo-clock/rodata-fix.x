/* Fix: merge .rodata_desc and .rodata into a single DROM segment.
   The padding BYTE(0) ensures this is PROGBITS (not NOBITS), so espflash
   won't split the DROM region at the alignment gap.
   See: https://github.com/esp-rs/esp-hal/pull/4844 */
SECTIONS {
  .rodata_merge : ALIGN(4) {
    BYTE(0)
    . = ALIGN(ALIGNOF(.rodata));
  } > drom_seg
}
INSERT BEFORE .rodata;
