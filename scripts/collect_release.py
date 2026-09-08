#!/usr/bin/env python3
import json
from pathlib import Path
import shutil
import sys
import tarfile
root=Path(__file__).resolve().parents[1]
platform=sys.argv[1]
version=json.loads((root/'package.json').read_text())['version']
out=root/'artifacts/release'/platform;out.mkdir(parents=True,exist_ok=True)
bundle=root/'target/release/bundle'
suffix='.app.tar.gz' if platform.startswith('darwin') else '.AppImage'
matches=list(bundle.rglob('*'+suffix))
if len(matches)!=1: raise SystemExit(f'Expected one {suffix} updater artifact, found {matches}')
source=matches[0]
name=f'Photo-Sorting-Hat-{version}-{platform}{suffix}'
shutil.copy2(source,out/name)
shutil.copy2(Path(str(source)+'.sig'),out/(name+'.sig'))
for dmg in bundle.rglob('*.dmg'): shutil.copy2(dmg,out/f'Photo-Sorting-Hat-{version}-{platform}.dmg')
with tarfile.open(out/f'photo-sorting-hat-cli-{version}-{platform}.tar.gz','w:gz') as tar:
    tar.add(root/'target/release/photo-sorting-hat',arcname='photo-sorting-hat')
    tar.add(root/'src-tauri/resources/metadata',arcname='metadata')
    tar.add(root/'LICENSE',arcname='LICENSE')
    tar.add(root/'README.md',arcname='README.md')
(out/'platform.json').write_text(json.dumps({'platform':platform,'artifact':name})+'\n')
