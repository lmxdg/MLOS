ENTRY(_start)

SECTIONS {
    /* 内核加载到 2 MiB，避免与 GRUB 自身（常驻 1 MiB 附近）冲突 */
    . = 2M;

    __kernel_start = .;

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

    /* 文件/磁盘镜像只需加载到这里；BSS 由内核启动代码清零。 */
    __kernel_data_end = .;

    .bss : ALIGN(8) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    __kernel_end = .;

    /DISCARD/ : {
        *(.eh_frame)
        *(.note.gnu.build-id)
        *(.comment)
        *(.note.GNU-stack)
    }
}
