"""Downscale the film captures in docs/demo/screenshots/{dark,light} to 1920x1200 JPEGs in ./img and ./img-light (deck fallbacks)."""
import glob, os
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__))
for mode, out in (('dark', 'img'), ('light', 'img-light')):
    src = os.path.join(HERE, '..', '..', 'demo', 'screenshots', mode)
    os.makedirs(os.path.join(HERE, out), exist_ok=True)
    for f in sorted(glob.glob(os.path.join(src, '*.png'))):
        Image.open(f).convert('RGB').resize((1920, 1200), Image.LANCZOS).save(os.path.join(HERE, out, os.path.basename(f)[:-4] + '.jpg'), quality=90, optimize=True)
print('done')
