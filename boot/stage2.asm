; MLOS disk boot - stage 2 (loaded at 0x0600 by MBR)
; Loads the MLOS kernel flat image from disk to 0x10000 via 1-sector EDD
; reads, builds a minimal Multiboot2 info (memory map from BIOS e820),
; enters 32-bit protected mode, copies the kernel to 0x200000 and jumps
; to its entry with EAX=multiboot2 magic, EBX=mbi. The kernel's own
; _start stub then sets up long mode and calls kernel_main.

%define E820_BUF   0x8000
%define MBI_BUF    0x9000
%define KFILE_BUF  0x10000
%define KERN_BASE  0x200000
%define HDR_LBA    4096
%define IMG_LBA    4097
%define MB2_MAGIC  0x36D76289

org 0x0600

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

    ; ---- collect e820 memory map (20-byte entries at 0x8000) ----
    mov di, E820_BUF
    xor ebx, ebx
    mov word [e820_count], 0
.e820_loop:
    mov edx, 0x534D4150          ; "SMAP"
    mov eax, 0xE820
    mov ecx, 20                  ; entry size
    int 15h
    jc .e820_done
    cmp eax, 0x534D4150
    jne .e820_done
    inc word [e820_count]
    add di, 20
    test ebx, ebx
    jz .e820_done
    cmp word [e820_count], 48
    jb .e820_loop
.e820_done:

    ; ---- read header block (LBA 4096) -> 0x1000 ----
    mov ax, 0x0000
    mov es, ax
    mov bx, 0x1000
    mov eax, HDR_LBA
    call read_one
    jc fail

    ; check magic "MLBX"
    mov eax, [0x1000]
    cmp eax, 0x58424C4D          ; little-endian 'M','L','B','X'
    jne fail

    mov eax, [0x1004]            ; image sectors
    test eax, eax
    jz fail
    cmp eax, 1120                ; must fit under 0xA0000 (~560 KiB)
    ja fail
    mov [img_sectors], eax

    ; ---- read kernel image (LBA 4097..) -> 0x10000, 1 sector per read ----
    mov esi, IMG_LBA
    mov edi, KFILE_BUF
    mov ecx, [img_sectors]
.img_loop:
    mov eax, edi                 ; flat dest -> ES:BX
    shr eax, 4
    mov es, ax
    mov bx, di
    and bx, 0xF
    mov eax, esi
    call read_one
    jc fail
    inc esi
    add edi, 512
    loop .img_loop

    ; ---- enable A20 (fast A20, port 0x92) ----
    in al, 0x92
    or al, 0x02
    and al, 0xFE
    out 0x92, al

    ; ---- enter protected mode ----
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
    mov esp, 0x7C00

    ; ---- copy kernel image 0x10000 -> 0x200000 ----
    mov esi, KFILE_BUF
    mov edi, KERN_BASE
    mov ecx, [img_sectors]
    shl ecx, 7                   ; sectors * 512 / 4
    cld
    rep movsd

    ; ---- build minimal Multiboot2 info at 0x9000 ----
    mov edi, MBI_BUF
    mov dword [edi+8], 6         ; tag type: memory map
    movzx eax, word [e820_count]
    imul eax, 24
    add eax, 16                  ; tag header
    mov [edi+12], eax            ; tag size
    mov dword [edi+16], 24       ; entry_size
    mov dword [edi+20], 0        ; entry_version
    add edi, 24
    ; copy e820 entries (20 -> 24 bytes each)
    mov esi, E820_BUF
    movzx ecx, word [e820_count]
.e_copy:
    movsd                        ; base low
    movsd                        ; base high
    movsd                        ; length low
    movsd                        ; length high
    movsd                        ; type
    xor eax, eax
    stosd                        ; reserved
    loop .e_copy
    ; end tag
    mov dword [edi], 0
    mov dword [edi+4], 8
    add edi, 8
    ; fix total size + reserved
    mov eax, edi
    sub eax, MBI_BUF
    mov dword [MBI_BUF], eax
    mov dword [MBI_BUF+4], 0

    ; ---- jump into the kernel ----
    mov ebx, MBI_BUF             ; EBX = multiboot2 info pointer
    mov edx, [0x1008]            ; entry offset from disk header
    add edx, KERN_BASE           ; EDX = kernel entry point
    mov eax, MB2_MAGIC           ; EAX = multiboot2 magic
    jmp edx

bits 16
; ---- read_one: read ONE sector at LBA EAX to ES:BX via EDD ----
; clobbers ax, si; keeps CF from int 13h
read_one:
    push ax
    push si
    mov si, dap
    mov byte [si+2], 1           ; 1 sector
    mov [si+4], bx               ; buffer offset
    mov [si+6], es               ; buffer segment
    mov [si+8], eax              ; LBA low
    mov dword [si+12], 0         ; LBA high
    mov ah, 0x42
    mov dl, [boot_drive]
    int 13h
    pop si
    pop ax
    ret

fail:
    xor ax, ax
    mov ds, ax
    mov si, msg_fail
.p:
    lodsb
    or al, al
    jz .h
    mov ah, 0x0E
    mov bx, 0x0007
    int 10h
    jmp .p
.h:
    cli
    hlt
    jmp .h

msg_fail db "MLOS boot: cannot load kernel from disk", 0

align 4
boot_drive   db 0
e820_count   dw 0
img_sectors  dd 0

; Disk Address Packet (used by read_one)
align 4
dap:
    db 0x10, 0
    dw 0                         ; sectors
    dw 0, 0                      ; buffer offset/segment
    dd 0                         ; LBA low
    dd 0                         ; LBA high

; 32-bit protected mode GDT: null / code (0x08) / data (0x10)
align 8
gdt:
    dq 0
    dq 0x00CF9A000000FFFF        ; code32: base 0, limit 4G
    dq 0x00CF92000000FFFF        ; data32: base 0, limit 4G
gdt_end:
gdt_ptr:
    dw gdt_end - gdt - 1
    dd gdt

times (16*1024)-($-$$) db 0    ; pad to 16 KiB (32 sectors)
