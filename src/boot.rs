// Multiboot2 引导头 + 32→64 位长模式切换存根
core::arch::global_asm!(
    r#"
    .section .multiboot, "a"
    .align 8
multiboot_header_start:
    .long 0xE85250D6              /* Multiboot2 header magic */
    .long 0                       /* architecture: i386 */
    .long multiboot_header_end - multiboot_header_start
    .long -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header_start))
    .short 0                      /* end tag */
    .short 0
    .long 8
multiboot_header_end:

    /* 页表：4KiB 对齐，identity map 低 4MiB（2 个 2MiB 大页） */
    .section .bss
    .align 4096
pml4:
    .skip 4096
pdpt:
    .skip 4096
pd:
    .skip 4096

    /* 内核栈 */
    .align 16
stack_bottom:
    .skip 16384
stack_top:

    /* 保存 multiboot2 参数 */
    .align 8
mb_magic:
    .quad 0
mb_info:
    .quad 0

    /* 64 位 GDT：null / code64(L=1) / data64 */
    .section .data
    .align 8
gdt64:
    .quad 0x0000000000000000
    .quad 0x00AF9A000000FFFF      /* 64-bit code, L=1, D=0 */
    .quad 0x00CF92000000FFFF      /* 64-bit data */
gdt64_end:
gdt64_pointer:
    .word gdt64_end - gdt64 - 1
    .quad gdt64

    /* 远跳转目标（32:16 far pointer） */
    .align 8
gdt64_target:
    .long start64
    .word 0x08                    /* code64 段选择子 */

    .section .text
    .code32
    .global _start
_start:
    /* 保存 multiboot2 参数（32 位模式下 EAX/EBX） */
    mov dword ptr [mb_magic], eax
    mov dword ptr [mb_info], ebx

    /* 构建页表：PML4[0] -> PDPT, PDPT[0] -> PD */
    mov eax, offset pdpt
    or eax, 3
    mov dword ptr [pml4], eax
    mov eax, offset pd
    or eax, 3
    mov dword ptr [pdpt], eax
    /* PD[0] = 0x000000 | 0x83 (2MiB huge page, PRESENT|RW|PS) */
    mov dword ptr [pd], 0x00000083
    mov dword ptr [pd + 4], 0
    /* PD[1] = 0x200000 | 0x83 */
    mov dword ptr [pd + 8], 0x00200083
    mov dword ptr [pd + 12], 0

    /* 加载 PML4 到 CR3 */
    mov eax, offset pml4
    mov cr3, eax

    /* CR4: PAE(bit5) | OSFXSR(bit9) | OSXMMEXCPT(bit10) */
    mov eax, cr4
    or eax, 0x620
    mov cr4, eax

    /* EFER.LME (bit 8) 启用 long mode */
    mov ecx, 0xC0000080
    rdmsr
    or eax, 0x100
    wrmsr

    /* CR0: PE(bit0) | PG(bit31) 启用分页 */
    mov eax, cr0
    or eax, 0x80000001
    mov cr0, eax

    /* 加载 64 位 GDT 并远跳转到 64 位代码段 */
    lgdt fword ptr [gdt64_pointer]
    jmp fword ptr [gdt64_target]

    .code64
start64:
    /* 设置 64 位数据段 */
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov rdi, qword ptr [mb_magic]
    mov rsi, qword ptr [mb_info]
    lea rsp, [stack_top]
    and rsp, -16
    call kernel_main
2:
    cli
    hlt
    jmp 2b
    "#
);
