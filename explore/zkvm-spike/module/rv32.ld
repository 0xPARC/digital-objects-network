/* Module image: the export table first, then code and data, at BASE. The
   interpreter maps the flat image at BASE and puts the stack at the top. */
MEMORY { RAM (rwx) : ORIGIN = 0x00010000, LENGTH = 0x00010000 }
ENTRY(craft_wood_pick)
SECTIONS {
  .exports : { KEEP(*(.exports)) } > RAM
  .text : { *(.text .text.*) } > RAM
  .rodata : { *(.srodata .srodata.* .rodata .rodata.*) } > RAM
  .data : { *(.sdata .sdata.* .data .data.*) } > RAM
  .bss (NOLOAD) : { *(.sbss .sbss.* .bss .bss.*) } > RAM
  /DISCARD/ : { *(.eh_frame .eh_frame_hdr .comment) }
}
