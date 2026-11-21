#!/usr/bin/env python3
"""生成 MLOS framebuffer 控制台使用的 8x16 ASCII 点阵字库。

输出 src/ascii_font_data.rs：
    pub static ASCII_GLYPHS: [[u8; 16]; 95]

索引 = 字符码 - 0x20（0x20..0x7E）。每行一个字节，最高位是最左侧像素。
仅在开发机运行，构建内核不需要 Pillow。
"""
import os

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONT_PATH = '/usr/share/fonts/dejavu/DejaVuSansMono-Bold.ttf'
OUT = os.path.join(ROOT, 'src', 'ascii_font_data.rs')

CELL_W, CELL_H = 8, 16
FONT_SIZE = 15
OFFSET_Y = -2
THRESHOLD = 96


def make_glyph(ch):
    img = Image.new('L', (CELL_W, CELL_H), 0)
    ImageDraw.Draw(img).text((0, OFFSET_Y), ch, font=ImageFont.truetype(FONT_PATH, FONT_SIZE), fill=255)
    pix = img.load()
    rows = []
    for y in range(CELL_H):
        b = 0
        for x in range(CELL_W):
            if pix[x, y] >= THRESHOLD:
                b |= 0x80 >> x
        rows.append(b)
    return rows


def main():
    glyphs = [make_glyph(chr(c)) for c in range(0x20, 0x7F)]
    out = [
        '// 自动生成：DejaVu Sans Mono Bold 8x16 ASCII 点阵字库。\n',
        '// 由 tools/gen_ascii_font.py 生成，请勿手动编辑。\n\n',
        'pub const FIRST: u8 = 0x20;\n',
        'pub const LAST: u8 = 0x7E;\n\n',
        'pub static ASCII_GLYPHS: [[u8; 16]; %d] = [\n' % len(glyphs),
    ]
    for c, rows in zip(range(0x20, 0x7F), glyphs):
        label = chr(c).replace('\\', '\\\\')
        out.append('    [' + ', '.join(f'0x{b:02X}' for b in rows) + f'], // 0x{c:02X} {label}\n')
    out.append('];\n')
    with open(OUT, 'w', encoding='utf-8') as fh:
        fh.write(''.join(out))
    print(f'已写入 {OUT}（{len(glyphs)} 个字形）')


if __name__ == '__main__':
    main()