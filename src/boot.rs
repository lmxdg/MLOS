// Multiboot2 引导头 + 32→64 位长模式切换存根
//
// 页表把低 4 GiB 全部用 2 MiB 大页恒等映射：VBE 线性帧缓冲通常位于
// 0xFD000000 附近，必须在内核进入长模式前就可访问。BSS（含页表、堆、
// 64 KiB 内核栈）在 32 位阶段先清零，再构建页表。
core::arch::global_asm!(
    r#"
    .section .multiboot, "a"
    .align 8
multiboot_header_start:
    .long 0xE85250D6              /* Multiboot2 header magic */
    .long 0                       /* architecture: i386 */
    .long multiboot_header_end - multiboot_header_start
    .long -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header_start))
    /* framebuffer request: 1024x768x32（GRUB 引导时使用） */
    .short 5                      /* type: framebuffer request */
    .short 0                      /* flags: optional */
    .long 20                      /* tag size */
    .long 1024
    .long 768
    .long 32
    .short 0                      /* end tag */
    .short 0
    .long 8
multiboot_header_end:

    /* 页表：4 个 PDPT + 4 个 PD，覆盖低 4 GiB（每项 2 MiB 大页） */
    .section .bss
    .align 4096
pml4:
    .skip 4096
pdpt:
    .skip 4096 * 4
pd:
    .skip 4096 * 4

    /* 内核栈 64 KiB（BSS 中，启动阶段被清零） */
    .align 16
stack_bottom:
    .skip 65536
stack_top:

    /* 保存 multiboot2 参数（.data，不会被 BSS 清零） */
    .section .data
    .align 8
mb_magic:
    .quad 0
mb_info:
    .quad 0

    /* 64 位 GDT：null / code64(L=1) / data64 */
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
    cli
    /* 保存 multiboot2 参数（32 位模式下 EAX/EBX） */
    mov dword ptr [mb_magic], eax
    mov dword ptr [mb_info], ebx
    mov dword ptr [mb_magic + 4], 0
    mov dword ptr [mb_info + 4], 0

    /* 清零 BSS（包含页表与内核栈） */
    mov edi, offset __bss_start
    mov ecx, offset __bss_end
    sub ecx, edi
    shr ecx, 2
    xor eax, eax
    cld
    rep stosd

    /* 构建页表：PD 中填入 2048 个 2 MiB 大页 */
    mov edi, offset pd
    mov eax, 0x83
    mov ecx, 2048
1:
    mov dword ptr [edi], eax
    mov dword ptr [edi + 4], 0
    add eax, 0x200000
    add edi, 8
    loop 1b

    /* PDPT[0..3] -> 四个 PD */
    mov eax, offset pd
    or eax, 3
    mov edi, offset pdpt
    mov ecx, 4
2:
    mov dword ptr [edi], eax
    mov dword ptr [edi + 4], 0
    add eax, 4096
    add edi, 8
    loop 2b

    /* PML4[0] -> PDPT */
    mov eax, offset pdpt
    or eax, 3
    mov dword ptr [pml4], eax
    mov dword ptr [pml4 + 4], 0

    /* 加载 PML4 到 CR3 */
    mov eax, offset pml4
    mov cr3, eax

    /* CR4: PAE(bit5) | PSE(bit4) | OSFXSR(bit9) | OSXMMEXCPT(bit10) */
    mov eax, cr4
    or eax, 0x630
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
    mov fs, ax
    mov gs, ax
    mov rdi, qword ptr [mb_magic]
    mov rsi, qword ptr [mb_info]
    lea rsp, [stack_top]
    and rsp, -16
    xor ebp, ebp
    call kernel_main
3:
    cli
    hlt
    jmp 3b
    "#
);