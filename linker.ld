ENTRY(_start)

SECTIONS {
    /* 内核加载到 2 MiB，避免与 GRUB 自身（常驻 1 MiB 附近）冲突 */
    . = 2M;

    .text : ALIGN(8) {
        KEEP(*(.multiboot))
        *(.text .text.*)
    }

    .rodata : ALIGN(8) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(8) {
        *(.data .data.*)
    }

    .bss : ALIGN(8) {
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.note.gnu.build-id)
        *(.comment)
        *(.note.GNU-stack)
    }
}
