"""Export desktop icons from the approved fish-school artwork (requires Pillow).

Only transparent padding and output size change; the artwork is not redrawn.
Run from any directory: python desktop/scripts/generate-app-icons.py
"""
from pathlib import Path
from PIL import Image

DESKTOP = Path(__file__).resolve().parents[1]
SOURCE = DESKTOP / 'src/assets/velella-logo-v2.png'
DEST = DESKTOP / 'src-tauri/icons'


def generate():
    image = Image.open(SOURCE).convert('RGBA')
    # Ignore nearly transparent export noise when measuring the visual mark.
    bounds = image.getchannel('A').point(lambda value: 255 if value >= 16 else 0).getbbox()
    if bounds is None:
        raise ValueError('Brand artwork is empty')
    mark = image.crop(bounds)
    edge = 1024
    mark.thumbnail((round(edge * .82), round(edge * .82)), Image.Resampling.LANCZOS)
    master = Image.new('RGBA', (edge, edge))
    master.alpha_composite(mark, ((edge-mark.width)//2, (edge-mark.height)//2))
    pngs = {'icon.png':512, '32x32.png':32, '64x64.png':64,
            '128x128.png':128, '128x128@2x.png':256, 'StoreLogo.png':50}
    for size in [30,44,71,89,107,142,150,284,310]:
        pngs[f'Square{size}x{size}Logo.png'] = size
    for name, size in pngs.items():
        master.resize((size,size), Image.Resampling.LANCZOS).save(DEST/name, optimize=True)
    master.save(DEST/'icon.ico', sizes=[(n,n) for n in [16,24,32,48,64,128,256]])
    master.save(DEST/'icon.icns')
    master.resize((512,512), Image.Resampling.LANCZOS).save(DESKTOP/'electron/icon.png', optimize=True)
    print('Exported Windows/macOS/Linux desktop icons from', SOURCE.name)


if __name__ == '__main__':
    generate()
