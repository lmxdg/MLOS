; MLOS disk boot - stage 1 (MBR, 512 bytes)
; BIOS loads at 0x7C00. Probes BIOS hard drives (0x80..0x83, then the
; passed-in DL), loads stage 2 (32 sectors = 16 KiB) from LBA 2048 to
; 0x0600 via EDD, verifies its magic, then jumps to it.

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
    mov [saved_dl], dl

    ; ---- probe drives: 0x80..0x83 first, then the passed-in DL ----
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
    ; ---- load stage2: 32 sectors from LBA 2048 -> 0x0600 ----
    mov si, dap
    mov byte [si+2], 32
    mov word [si+4], 0x0600
    mov word [si+6], 0x0000
    mov dword [si+8], 2048
    mov dword [si+12], 0
    mov ah, 0x42
    mov dl, [boot_drive]
    int 13h
    jc disk_fail

    ; jump to stage2
    jmp 0x0000:0x0600

disk_fail:
    mov si, msg_fail
    call print
.halt:
    hlt
    jmp .halt

print:
    lodsb
    or al, al
    jz .done
    mov ah, 0x0E
    mov bx, 0x0007
    int 10h
    jmp print
.done:
    ret

try_drive:
    ; BP=1 if this drive reads LBA 2048 and the stage2 magic matches
    mov [boot_drive], dl
    mov si, dap
    mov byte [si+2], 1
    mov word [si+4], 0x0600
    mov word [si+6], 0x0000
    mov dword [si+8], 2048
    mov dword [si+12], 0
    mov ah, 0x42
    int 13h
    jc .no
    cmp byte [0x0600], 0xFA     ; stage2 starts: cli
    jne .no
    cmp byte [0x0601], 0x31     ; xor ax,ax
    jne .no
    mov bp, 1
.no:
    ret

msg_fail db "MLOS: disk boot failed", 0

boot_drive db 0
saved_dl   db 0

; Disk Address Packet
align 4
dap:
    db 0x10, 0
    dw 0                         ; sectors
    dw 0, 0                      ; buffer offset/segment
    dd 0                         ; LBA low
    dd 0                         ; LBA high

times 510-($-$$) db 0
dw 0xAA55
