; MLOS 硬盘自引导 - Stage 1（MBR, 512 字节）
; BIOS 加载到 0x7C00。依次探测 BIOS 硬盘（0x80..0x83，然后 DL），
; 从 LBA 2048 读取 64 个扇区（32 KiB）的 stage2 到 0x7E00，
; 校验起始指令字后跳转。

org 0x7C00
bits 16

STAGE2_SEG   equ 0x0000
STAGE2_OFF   equ 0x7E00
STAGE2_LBA   equ 2048
STAGE2_SECT  equ 64

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti
    mov [saved_dl], dl

    mov bp, 0
    mov dl, 0x80
.probe_loop:
    call try_drive
    test bp, bp
    jnz drive_ok
    inc dl
    cmp dl, 0x84
    jb .probe_loop
    mov dl, [saved_dl]
    call try_drive
    test bp, bp
    jnz drive_ok
    jmp disk_fail

drive_ok:
    mov si, dap
    mov byte [si + 2], STAGE2_SECT
    mov word [si + 4], STAGE2_OFF
    mov word [si + 6], STAGE2_SEG
    mov dword [si + 8], STAGE2_LBA
    mov dword [si + 12], 0
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    jc disk_fail
    jmp STAGE2_SEG:STAGE2_OFF

disk_fail:
    mov si, msg_fail
    call print
.halt:
    cli
    hlt
    jmp .halt

print:
    lodsb
    or al, al
    jz .done
    mov ah, 0x0E
    mov bx, 0x0007
    int 0x10
    jmp print
.done:
    ret

try_drive:
    mov [boot_drive], dl
    mov si, dap
    mov byte [si + 2], 1
    mov word [si + 4], STAGE2_OFF
    mov word [si + 6], STAGE2_SEG
    mov dword [si + 8], STAGE2_LBA
    mov dword [si + 12], 0
    mov ah, 0x42
    int 0x13
    jc .no
    cmp byte [STAGE2_OFF], 0xFA       ; cli
    jne .no
    cmp byte [STAGE2_OFF + 1], 0x31   ; xor ax, ax
    jne .no
    mov bp, 1
.no:
    ret

msg_fail db "MLOS: disk boot failed", 0

boot_drive db 0
saved_dl   db 0

align 4
dap:
    db 0x10, 0
    dw 0
    dw 0, 0
    dd 0
    dd 0

times 510 - ($ - $$) db 0
dw 0xAA55
