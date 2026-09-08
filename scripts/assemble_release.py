#!/usr/bin/env python3
"""Build a combined Tauri updater manifest from per-architecture signed artifacts."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
from urllib.parse import quote
p=argparse.ArgumentParser();p.add_argument('directory',type=Path);p.add_argument('--repository',required=True);p.add_argument('--version',required=True);args=p.parse_args()
platforms={}
for path in args.directory.rglob('platform.json'):
    record=json.loads(path.read_text())
    artifact=path.parent/record['artifact']
    sig=artifact.with_name(artifact.name+'.sig')
    if not artifact.is_file() or not sig.is_file(): raise SystemExit(f'Missing signed artifact: {artifact}')
    platforms[record['platform']]={'signature':sig.read_text().strip(),'url':f'https://github.com/{args.repository}/releases/download/v{args.version}/{quote(artifact.name)}'}
expected={'darwin-aarch64','darwin-x86_64','linux-x86_64'}
if set(platforms)!=expected: raise SystemExit(f'Expected {expected}, got {set(platforms)}')
manifest={'version':args.version,'notes':'Photo Sorting Hat preview. See the release notes and compatibility matrix.','pub_date':datetime.now(timezone.utc).isoformat(),'platforms':platforms}
(args.directory/'latest.json').write_text(json.dumps(manifest,indent=2)+'\n')
