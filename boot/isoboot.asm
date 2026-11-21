; ===========================================================================
; MLOS ISO El Torito 引导镜像（no-emulation，BIOS 加载到 0x7C00）
; ===========================================================================
; 镜像尾部（由 tools/mkiso.py 追加）：
;   文件偏移 0x5400 -> 物理 0xD000 : 512 字节内核头 "MLBX"
;   文件偏移 0x5600 -> 物理 0xD200 : 内核扁平镜像
; 本程序设置 VBE 图形模式、构建 Multiboot2 信息（内存图 + framebuffer），
; 把内核复制到 0x200000 后跳转，无需 GRUB。
; ===========================================================================

%define E820_BUF   0x3000
%define MBI_BUF    0x5000
%define HDR_ADDR   0xD000
%define KFILE_ADDR 0xD200
%define KERN_BASE  0x200000
%define MB2_MAGIC  0x36D76289

org 0x7C00

bits 16
start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti
    cld

    ; ---- e820 ----
    mov edi, E820_BUF
    xor ebx, ebx
    mov word [e820_count], 0
.e820_loop:
    mov edx, 0x534D4150
    mov eax, 0xE820
    mov ecx, 20
    int 0x15
    jc .e820_done
    cmp eax, 0x534D4150
    jne .e820_done
    inc word [e820_count]
    add edi, 20
    test ebx, ebx
    jz .e820_done
    cmp word [e820_count], 48
    jb .e820_loop
.e820_done:

    ; ---- VBE ----
    call vbe_setup

    ; ---- 校验镜像内嵌的内核头 ----
    cmp dword [HDR_ADDR], 0x58424C4D
    jne fail
    mov eax, [HDR_ADDR + 4]
    test eax, eax
    jz fail
    cmp eax, 1120
    ja fail
    mov [img_sectors], eax

    ; ---- 构建 Multiboot2 信息 ----
    xor ax, ax
    mov es, ax
    mov edi, MBI_BUF
    mov dword [edi + 8], 6
    movzx eax, word [e820_count]
    imul eax, 24
    add eax, 16
    mov [edi + 12], eax
    mov dword [edi + 16], 24
    mov dword [edi + 20], 0
    add edi, 24
    mov esi, E820_BUF
    movzx ecx, word [e820_count]
.e_copy:
    test ecx, ecx
    jz .e_copy_done
    movsd
    movsd
    movsd
    movsd
    movsd
    xor eax, eax
    stosd
    dec ecx
    jmp .e_copy
.e_copy_done:
    call vbe_build_tag
    mov dword [edi], 0
    mov dword [edi + 4], 8
    add edi, 8
    mov eax, edi
    sub eax, MBI_BUF
    mov [MBI_BUF], eax
    mov dword [MBI_BUF + 4], 0

    ; ---- A20 + 保护模式 ----
    in al, 0x92
    or al, 0x02
    and al, 0xFE
    out 0x92, al

    cli
    lgdt [gdt_ptr]
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    jmp 0x08:pm_start

bits 32
pm_start:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov esi, KFILE_ADDR
    mov edi, KERN_BASE
    mov ecx, [img_sectors]
    shl ecx, 7
    cld
    rep movsd
    mov esp, 0x70000

    mov ebx, MBI_BUF
    mov edx, [HDR_ADDR + 8]
    add edx, KERN_BASE
    mov eax, MB2_MAGIC
    jmp edx

bits 16
%include "vbe.inc"

fail:
    mov si, msg_fail
.p:
    lodsb
    or al, al
    jz .h
    mov ah, 0x0E
    mov bx, 0x0007
    int 0x10
    jmp .p
.h:
    cli
    hlt
    jmp .h

msg_fail db "MLOS boot: invalid ISO kernel image", 0

align 4
e820_count  dw 0
img_sectors dd 0

align 8
gdt:
    dq 0
    dq 0x00CF9A000000FFFF
    dq 0x00CF92000000FFFF
gdt_end:
gdt_ptr:
    dw gdt_end - gdt - 1
    dd gdt

times 0x5400 - ($ - $$) db 0