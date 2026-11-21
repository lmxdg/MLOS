#!/usr/bin/env python3
"""生成 MLOS 中文预览版点阵字库 src/zhfont_data.rs。

扫描 src/*.rs 的字符串字面量，收集其中出现的汉字，用 Noto Sans CJK SC
渲染为 16x16 点阵，拆成左右两个 8x16 字形（32 字节/字），按 Unicode
排序输出，便于内核二分查找。

依赖：Pillow（PIL）与 Noto CJK 字体。仅在开发机运行，构建内核不需要。
"""
import collections
import glob
import os

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONT_PATH = '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'
FONT_INDEX = 2  # Simplified Chinese face in the TTC
OUT = os.path.join(ROOT, 'src', 'zhfont_data.rs')


def collect_chars():
    text = ''
    for f in glob.glob(os.path.join(ROOT, 'src', '*.rs')):
        with open(f, encoding='utf-8') as fh:
            for line in fh:
                s = line.strip()
                if s.startswith('//!') or s.startswith('//'):
                    continue
                text += line
    # 只保留字符串字面量内的汉字（粗粒度：非注释行）
    return collections.Counter(c for c in text if '\u4e00' <= c <= '\u9fff')


def main():
    chars = collect_chars()
    ordered = [c for c, _ in chars.most_common()]
    print(f'字库汉字数: {len(ordered)}')

    font = ImageFont.truetype(FONT_PATH, 16, index=FONT_INDEX)
    glyphs = []
    for ch in ordered:
        img = Image.new('L', (16, 16), 0)
        ImageDraw.Draw(img).text((0, -1), ch, font=font, fill=255)
        pix = img.load()
        data = []
        for half in (0, 8):
            for y in range(16):
                byte = 0
                for x in range(8):
                    if pix[half + x, y] >= 128:
                        byte |= 0x80 >> x
                data.append(byte)
        glyphs.append((ch, bytes(data[:32])))

    glyphs.sort(key=lambda t: ord(t[0]))
    out = [
        '// 自动生成：Noto Sans CJK SC 16x16 点阵，每字拆为左右两个 8x16 字形（32 字节）。\n',
        '// 由 tools/gen_zhfont.py 生成，请勿手动编辑。\n\n',
        'pub static ZH_CHARS: [u32; %d] = [\n' % len(glyphs),
        '    ' + ', '.join(f'0x{ord(c):04X}' for c, _ in glyphs) + '\n];\n\n',
        '/// 按出现频次排序的热字码点，用于开机预热字模槽。\n',
        'pub static ZH_HOT: [u32; 64] = [\n',
        '    ' + ', '.join(f'0x{ord(c):04X}' for c in ordered[:64]) + '\n];\n\n',
        'pub static ZH_GLYPHS: [[u8; 32]; %d] = [\n' % len(glyphs),
    ]
    for ch, g in glyphs:
        out.append('    [' + ', '.join(f'0x{b:02X}' for b in g) + f'], // {ch}\n')
    out.append('];\n\n')
    out.append('''pub fn index_of(cp: u32) -> Option<usize> {
    let mut lo = 0usize;
    let mut hi = ZH_CHARS.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        match ZH_CHARS[mid].cmp(&cp) {
            core::cmp::Ordering::Less => lo = mid + 1,
            core::cmp::Ordering::Greater => hi = mid,
            core::cmp::Ordering::Equal => return Some(mid),
        }
    }
    None
}
''')
    with open(OUT, 'w', encoding='utf-8') as fh:
        fh.write(''.join(out))
    print(f'已写入 {OUT}')


if __name__ == '__main__':
    main()
