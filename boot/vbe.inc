; ===========================================================================
; MLOS VBE 2.0 线性帧缓冲设置（16 位实模式，供 BIOS 引导程序包含）
; ===========================================================================
; 调用 vbe_setup 后：
;   fb_present = 1 表示已切换到可用的线性图形模式
;   fb_addr / fb_pitch / fb_width / fb_height / fb_bpp 保存模式参数
; 失败时保持原文本模式，fb_present = 0。
; ===========================================================================

%ifndef MLOS_VBE_INC
%define MLOS_VBE_INC

VBE_INFO_BUF   equ 0x4100
VBE_MODE_BUF   equ 0x4000

vbe_setup:
    pusha
    push es
    xor ax, ax
    mov es, ax
    mov di, VBE_INFO_BUF
    mov ax, 0x4F00
    int 0x10
    cmp ax, 0x004F
    jne .fail
    cmp dword [VBE_INFO_BUF], 0x41534556      ; 'VESA'
    jne .fail

    mov si, vbe_modes
.next_mode:
    mov cx, [si]
    add si, 2
    cmp cx, 0xFFFF
    je .fail
    mov ax, 0x4F01
    mov di, VBE_MODE_BUF
    int 0x10
    cmp ax, 0x004F
    jne .next_mode
    mov dx, [VBE_MODE_BUF]                    ; ModeAttributes
    and dx, 0x0081                            ; supported + linear framebuffer
    cmp dx, 0x0081
    jne .next_mode
    mov al, [VBE_MODE_BUF + 0x19]             ; BitsPerPixel
    cmp al, 32
    je .mode_ok
    cmp al, 24
    je .mode_ok
    jmp .next_mode
.mode_ok:
    mov bx, cx
    or bx, 0x4000                             ; use linear framebuffer
    mov ax, 0x4F02
    int 0x10
    cmp ax, 0x004F
    jne .next_mode

    mov [fb_mode], cx
    mov ax, [VBE_MODE_BUF + 0x12]             ; XResolution
    mov [fb_width], ax
    mov ax, [VBE_MODE_BUF + 0x14]             ; YResolution
    mov [fb_height], ax
    mov ax, [VBE_MODE_BUF + 0x10]             ; BytesPerScanLine
    mov [fb_pitch], ax
    mov al, [VBE_MODE_BUF + 0x19]             ; BitsPerPixel
    mov [fb_bpp], al
    mov eax, [VBE_MODE_BUF + 0x28]            ; PhysBasePtr
    mov [fb_addr], eax

    ; 基本合理性检查
    cmp word [fb_width], 640
    jb .fail
    cmp word [fb_height], 480
    jb .fail
    test eax, eax
    jz .fail
    mov byte [fb_present], 1
    jmp .done

.fail:
    mov byte [fb_present], 0
    ; 恢复标准 80x25 文本模式
    mov ax, 0x0003
    int 0x10
.done:
    pop es
    popa
    ret

; 在 MBI 中追加 Multiboot2 framebuffer tag（type 8, size 40）
; 输入：EDI = 当前 tag 写入位置；输出：EDI 前进 40 字节
vbe_build_tag:
    cmp byte [fb_present], 0
    je .ret
    mov dword [edi], 8
    mov dword [edi + 4], 40
    mov eax, [fb_addr]
    mov [edi + 8], eax
    mov dword [edi + 12], 0
    movzx eax, word [fb_pitch]
    mov [edi + 16], eax
    movzx eax, word [fb_width]
    mov [edi + 20], eax
    movzx eax, word [fb_height]
    mov [edi + 24], eax
    mov al, [fb_bpp]
    mov [edi + 28], al
    mov byte [edi + 29], 1                    ; RGB
    mov word [edi + 30], 0
    mov byte [edi + 32], 16                   ; red position
    mov byte [edi + 33], 8                    ; red mask size
    mov byte [edi + 34], 8                    ; green position
    mov byte [edi + 35], 8                    ; green mask size
    mov byte [edi + 36], 0                    ; blue position
    mov byte [edi + 37], 8                    ; blue mask size
    mov word [edi + 38], 0
    add edi, 40
.ret:
    ret

; 优先 1024x768x32，再尝试 800x600x32 / 640x480x32
vbe_modes:
    dw 0x0118
    dw 0x0115
    dw 0x0112
    dw 0xFFFF

fb_present db 0
fb_mode    dw 0
fb_addr    dd 0
fb_pitch   dw 0
fb_width   dw 0
fb_height  dw 0
fb_bpp     db 0

%endif