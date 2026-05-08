/* RP2350A / Pico 2 W memory layout.
 *
 * 4 MB QSPI flash mapped at 0x10000000.
 * 512 KB striped SRAM at 0x20000000 plus two 4 KB direct-mapped banks
 * (SRAM8/SRAM9) at 0x20080000 / 0x20081000.
 *
 * The .start_block section near the top of FLASH is reserved for the RP2350
 * Image Definition (IMAGE_DEF) header. embassy-rp's default
 * `imagedef-secure-exe` feature places a valid IMAGE_DEF there automatically;
 * RP2350 will refuse to boot without one in the first 4 KB of the image.
 */

MEMORY {
    FLASH : ORIGIN = 0x10000000, LENGTH = 4096K
    RAM   : ORIGIN = 0x20000000, LENGTH = 512K
    SRAM8 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM9 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH
} INSERT AFTER .vector_table;

_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    .bi_entries : ALIGN(4)
    {
        __bi_entries_start = .;
        KEEP(*(.bi_entries));
        . = ALIGN(4);
        __bi_entries_end = .;
    } > FLASH

    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH
} INSERT AFTER .text;
