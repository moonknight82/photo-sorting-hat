#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
root=Path(__file__).resolve().parents[1]
directory=Path(sys.argv[1]).resolve()
version=json.loads((root/'package.json').read_text())['version']
tag='v'+version
if os.environ.get('GITHUB_REF_TYPE')=='tag' and os.environ.get('GITHUB_REF_NAME')!=tag:
    raise SystemExit('Tag and application version differ')
repo=os.environ['GITHUB_REPOSITORY']
subprocess.run([sys.executable,str(root/'scripts/assemble_release.py'),str(directory),'--repository',repo,'--version',version],check=True)
notes=root/'docs/RELEASE_NOTES.md'
subprocess.run(['gh','release','create',tag,'--repo',repo,'--draft','--title',f'Photo Sorting Hat {version} — Preview','--notes-file',str(notes),'--target',os.environ['GITHUB_SHA']],check=True)
files=[str(p) for p in directory.rglob('*') if p.is_file() and p.name!='platform.json']
subprocess.run(['gh','release','upload',tag,'--repo',repo,*files],check=True)
subprocess.run(['gh','release','edit',tag,'--repo',repo,'--draft=false','--latest'],check=True)
