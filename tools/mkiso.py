#!/usr/bin/env python3
"""把 MLOS 内核打包成可引导 ISO（ISO9660 + El Torito no-emulation）。

不依赖 GRUB / xorriso：
  - 解析内核 ELF 的 PT_LOAD 段，重建内核运行时的扁平镜像；
  - 把 boot/isoboot.bin（已设置 VBE + 构建 Multiboot2 信息）与
    512 字节内核头、内核镜像拼成 El Torito 引导镜像；
  - 用纯 Python 的 pycdlib 生成 ISO9660/Joliet ISO，并写入 El Torito 目录。

用法：
  python3 tools/mkiso.py <kernel-elf> <isoboot.bin> <output.iso>
"""
import io
import os
import struct
import sys

import pycdlib

KERNEL_BASE = 0x200000
BOOT_HDR_OFF = 0x5400
BOOT_IMG_OFF = 0x5600
MAX_IMAGE_SECTORS = 1120


def parse_elf(elf):
    if elf[:4] != b'\x7fELF' or elf[4] != 2:
        raise SystemExit('kernel is not a 64-bit ELF')
    e_phoff = struct.unpack_from('<Q', elf, 0x20)[0]
    e_phentsize = struct.unpack_from('<H', elf, 0x36)[0]
    e_phnum = struct.unpack_from('<H', elf, 0x38)[0]
    loads = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type = struct.unpack_from('<I', elf, off)[0]
        if p_type != 1:
            continue
        p_offset = struct.unpack_from('<Q', elf, off + 8)[0]
        p_vaddr = struct.unpack_from('<Q', elf, off + 16)[0]
        p_filesz = struct.unpack_from('<Q', elf, off + 32)[0]
        p_memsz = struct.unpack_from('<Q', elf, off + 40)[0]
        loads.append((p_vaddr, p_offset, p_filesz, p_memsz))
    return loads


def find_symbol(elf, name):
    e_shoff = struct.unpack_from('<Q', elf, 0x28)[0]
    e_shentsize = struct.unpack_from('<H', elf, 0x3A)[0]
    e_shnum = struct.unpack_from('<H', elf, 0x3C)[0]
    sections = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        sh_type = struct.unpack_from('<I', elf, off + 4)[0]
        sh_offset = struct.unpack_from('<Q', elf, off + 24)[0]
        sh_size = struct.unpack_from('<Q', elf, off + 32)[0]
        sh_link = struct.unpack_from('<I', elf, off + 40)[0]
        sh_entsize = struct.unpack_from('<Q', elf, off + 56)[0]
        sections.append((sh_type, sh_offset, sh_size, sh_link, sh_entsize))
    for sh_type, sh_offset, sh_size, sh_link, sh_entsize in sections:
        if sh_type != 2 or sh_entsize == 0:
            continue
        str_off, str_size = sections[sh_link][1], sections[sh_link][2]
        for pos in range(sh_offset, sh_offset + sh_size, sh_entsize):
            st_name = struct.unpack_from('<I', elf, pos)[0]
            st_value = struct.unpack_from('<Q', elf, pos + 8)[0]
            end = elf.find(b'\0', str_off + st_name)
            sym = elf[str_off + st_name:end].decode('ascii', 'ignore')
            if sym == name:
                return st_value
    return None


def flat_image(elf, loads):
    base = min(v for v, _, _, _ in loads)
    end = max(v + f for v, _, f, _ in loads)
    image = bytearray(end - base)
    for vaddr, offset, filesz, _memsz in loads:
        start = vaddr - base
        image[start:start + filesz] = elf[offset:offset + filesz]
    return base, bytes(image)


def build_boot_image(elf, isoboot):
    loads = parse_elf(elf)
    base, image = flat_image(elf, loads)
    if base != KERNEL_BASE:
        raise SystemExit(f'unexpected kernel base 0x{base:X}')
    start = find_symbol(elf, '_start')
    if start is None:
        raise SystemExit('cannot find _start symbol')
    entry_off = start - base
    sectors = (len(image) + 511) // 512
    if sectors == 0 or sectors > MAX_IMAGE_SECTORS:
        raise SystemExit(f'kernel image too large: {sectors} sectors')
    image = image + b'\0' * (sectors * 512 - len(image))

    if len(isoboot) > BOOT_HDR_OFF:
        raise SystemExit(f'isoboot.bin too large: {len(isoboot)} > {BOOT_HDR_OFF}')
    boot = bytearray(isoboot)
    boot += b'\0' * (BOOT_HDR_OFF - len(boot))

    header = bytearray(512)
    header[0:4] = b'MLBX'
    struct.pack_into('<I', header, 4, sectors)
    struct.pack_into('<I', header, 8, entry_off)
    boot += header
    boot += image
    return bytes(boot), len(image), entry_off


def main():
    if len(sys.argv) != 4:
        print(__doc__)
        raise SystemExit(2)
    kernel_path, isoboot_path, out_path = sys.argv[1:4]
    elf = open(kernel_path, 'rb').read()
    isoboot = open(isoboot_path, 'rb').read()
    boot, image_len, entry_off = build_boot_image(elf, isoboot)
    print(f'kernel flat image : {image_len} bytes, entry offset 0x{entry_off:X}')
    print(f'El Torito image   : {len(boot)} bytes ({len(boot)//512} sectors)')

    readme = (
        b'MLOS - Minimal Light Operating System\r\n'
        b'Version 1.0.6 (x86_64, Rust no_std)\r\n'
        b'\r\n'
        b'Boot this ISO in BIOS/CSM mode (legacy boot).\r\n'
        b'First boot runs the setup wizard and checks:\r\n'
        b'  - x86_64 CPU with PAE/SSE2, 16 MiB RAM\r\n'
        b'  - 16 MiB ATA disk, PS/2 keyboard\r\n'
        b'  - VBE linear framebuffer for the GUI (text fallback otherwise)\r\n'
        b'\r\n'
        b'Developer: MEME-LMXDG\r\n'
        b'https://github.com/lmxdg/MLOS\r\n'
    )

    iso = pycdlib.PyCdlib()
    iso.new(interchange_level=3, joliet=3, rock_ridge='1.09', vol_ident='MLOS')
    iso.add_fp(io.BytesIO(readme), len(readme), '/README.TXT;1',
               rr_name='README.TXT', joliet_path='/README.TXT')
    iso.add_directory('/BOOT', rr_name='BOOT', joliet_path='/boot')
    iso.add_fp(io.BytesIO(boot), len(boot), '/BOOT/MLOS.IMG;1',
               rr_name='MLOS.IMG', joliet_path='/boot/mlos.img')
    iso.add_eltorito('/BOOT/MLOS.IMG;1', bootcatfile='/BOOT/BOOT.CAT;1',
                     rr_bootcatname='BOOT.CAT', joliet_bootcatfile='/boot/boot.cat',
                     media_name='noemul', platform_id=0)
    iso.write(out_path)
    iso.close()
    print(f'已生成 {out_path} ({os.path.getsize(out_path)} bytes)')


if __name__ == '__main__':
    main()