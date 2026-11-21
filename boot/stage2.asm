; ===========================================================================
; MLOS 硬盘自引导 - Stage 2（由 MBR 加载到 0x7E00）
; ===========================================================================
; 1. 读取 BIOS e820 内存图到 0x3000
; 2. 通过 VBE 设置线性 framebuffer（1024x768x32，失败则保留文本模式）
; 3. 从 LBA 4096 读内核头、LBA 4097 起读内核扁平镜像到 0x10000
; 4. 在 0x5000 构建 Multiboot2 信息（内存图 + framebuffer tag）
; 5. 进入 32 位保护模式，把内核复制到 0x200000 并跳转
; ===========================================================================

%define E820_BUF   0x3000
%define HDR_BUF    0x0500
%define KFILE_BUF  0x10000
%define MBI_BUF    0x5000
%define KERN_BASE  0x200000
%define HDR_LBA    4096
%define IMG_LBA    4097
%define MB2_MAGIC  0x36D76289

org 0x7E00

bits 16
start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti
    mov [boot_drive], dl
    cld

    ; ---- 读取 e820 内存图 ----
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

    ; ---- VBE 图形模式 ----
    call vbe_setup

    ; ---- 读内核头（LBA 4096 -> 0x0500） ----
    xor ax, ax
    mov es, ax
    mov bx, HDR_BUF
    mov eax, HDR_LBA
    call read_one
    jc fail
    cmp dword [HDR_BUF], 0x58424C4D       ; "MLBX"
    jne fail
    mov eax, [HDR_BUF + 4]                ; 镜像扇区数
    test eax, eax
    jz fail
    cmp eax, 1120                         ; 560 KiB 上限
    ja fail
    mov [img_sectors], eax

    ; ---- 读内核镜像到 0x10000 ----
    mov esi, IMG_LBA
    mov edi, KFILE_BUF
    mov ecx, [img_sectors]
.img_loop:
    mov eax, edi
    shr eax, 4
    mov es, ax
    mov bx, di
    and bx, 0x0F
    mov eax, esi
    call read_one
    jc fail
    inc esi
    add edi, 512
    loop .img_loop

    ; ---- 构建 Multiboot2 信息 ----
    xor ax, ax
    mov es, ax
    mov edi, MBI_BUF
    mov dword [edi + 8], 6                ; memory map tag
    movzx eax, word [e820_count]
    imul eax, 24
    add eax, 16
    mov [edi + 12], eax
    mov dword [edi + 16], 24              ; entry_size
    mov dword [edi + 20], 0               ; entry_version
    add edi, 24
    mov esi, E820_BUF
    movzx ecx, word [e820_count]
.e_copy:
    test ecx, ecx
    jz .e_copy_done
    movsd                                 ; base low
    movsd                                 ; base high
    movsd                                 ; length low
    movsd                                 ; length high
    movsd                                 ; type
    xor eax, eax
    stosd                                 ; reserved
    dec ecx
    jmp .e_copy
.e_copy_done:
    call vbe_build_tag                    ; EDI 前进 40 字节（若有 framebuffer）
    mov dword [edi], 0                    ; end tag
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

    ; 复制内核镜像 0x10000 -> 0x200000
    mov esi, KFILE_BUF
    mov edi, KERN_BASE
    mov ecx, [img_sectors]
    shl ecx, 7                             ; sectors * 512 / 4
    cld
    rep movsd
    mov esp, 0x70000

    mov ebx, MBI_BUF
    mov edx, [HDR_BUF + 8]                 ; entry offset
    add edx, KERN_BASE
    mov eax, MB2_MAGIC
    jmp edx

bits 16
; ---- 读取一个扇区：LBA=EAX -> ES:BX（EDD） ----
read_one:
    push eax
    push si
    mov si, dap
    mov byte [si + 2], 1
    mov [si + 4], bx
    mov [si + 6], es
    mov [si + 8], eax
    mov dword [si + 12], 0
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    pop si
    pop eax
    ret

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

msg_fail db "MLOS boot: cannot load kernel from disk", 0

align 4
boot_drive  db 0
e820_count  dw 0
img_sectors dd 0

align 4
dap:
    db 0x10, 0
    dw 0
    dw 0, 0
    dd 0
    dd 0

align 8
gdt:
    dq 0
    dq 0x00CF9A000000FFFF
    dq 0x00CF92000000FFFF
gdt_end:
gdt_ptr:
    dw gdt_end - gdt - 1
    dd gdt

times (32*1024) - ($ - $$) db 0