#!/usr/bin/env python3
"""Embed only public release information. Private signing keys stay in CI secrets."""
import json
import os
from pathlib import Path
root = Path(__file__).resolve().parents[1]
repository = os.environ['GITHUB_REPOSITORY']
key = os.environ['TAURI_SIGNING_PUBLIC_KEY'].strip()
if not key or repository.count('/') != 1:
    raise SystemExit('Set GITHUB_REPOSITORY and TAURI_SIGNING_PUBLIC_KEY')
endpoint = f'https://github.com/{repository}/releases/latest/download/latest.json'
(root/'src-tauri/release.json').write_text(json.dumps({'endpoint':endpoint,'pubkey':key},indent=2)+'\n')
(root/'src-tauri/updater-build.json').write_text(json.dumps({
    'bundle': {'createUpdaterArtifacts': True},
    'plugins': {'updater': {'pubkey': key, 'endpoints': [endpoint]}},
},indent=2)+'\n')
