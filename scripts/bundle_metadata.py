#!/usr/bin/env python3
"""Bundle installed ExifTool and Perl, including core modules and native dependencies.

Run on the target architecture. Nothing is downloaded or installed by this script.
"""
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / 'src-tauri/resources/metadata'

def run(*args):
    return subprocess.check_output(args, text=True).strip()

def copy_tree(source, destination):
    source, destination = Path(source), Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    for child in source.iterdir():
        if child.is_dir(): copy_tree(child, destination / child.name)
        else: shutil.copy(child, destination / child.name)

def main():
    exiftool = Path(shutil.which('exiftool') or '/not-installed').resolve()
    if not exiftool.is_file():
        raise SystemExit('Install ExifTool before bundling.')
    if (exiftool.parent.parent / 'libexec/bin/exiftool').is_file():
        exiftool = exiftool.parent.parent / 'libexec/bin/exiftool'
    shebang = exiftool.read_text().splitlines()[0].removeprefix('#!')
    perl = Path(shebang if Path(shebang).is_file() else shutil.which('perl')).resolve()
    privlib, archlib = run(str(perl), '-MConfig', '-e', 'print "$Config{privlib}\\n$Config{archlib}"').splitlines()
    candidates = [exiftool.parent / 'lib', exiftool.parent.parent / 'lib/perl5', Path('/usr/share/perl5')]
    library = next((p for p in candidates if (p / 'Image/ExifTool.pm').is_file()), None)
    if library is None:
        raise SystemExit('Cannot locate Image/ExifTool.pm; unsupported installation layout.')
    if DEST.exists(): shutil.rmtree(DEST)
    DEST.mkdir(parents=True)
    shutil.copy(perl, DEST / 'perl')
    shutil.copy(exiftool, DEST / 'exiftool')
    copy_tree(library / 'Image', DEST / 'lib/Image')
    if (library / 'File').exists(): copy_tree(library / 'File', DEST / 'lib/File')
    # Keep separately named module roots so PERL5LIB does not depend on host paths.
    copy_tree(privlib, DEST / 'perl-lib')
    copy_tree(archlib, DEST / 'perl-arch')
    native = DEST / 'native'; native.mkdir()
    if platform.system() == 'Darwin':
        old_lib = str(Path(archlib)/'CORE/libperl.dylib')
        subprocess.run(['install_name_tool','-change',old_lib,'@executable_path/perl-arch/CORE/libperl.dylib',str(DEST/'perl')],check=True)
        for binary in [*(DEST/'perl-arch').rglob('*.bundle'), *(DEST/'perl-arch').rglob('*.dylib'), DEST/'perl']:
            subprocess.run(['codesign','--force','--sign','-','--timestamp=none',str(binary)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    if platform.system() == 'Linux':
        # Perl may dynamically link libperl/libcrypt. Include these while using the OS libc/loader.
        for line in run('ldd', str(perl)).splitlines():
            if '=>' not in line: continue
            dep = line.split('=>', 1)[1].strip().split()[0]
            if dep.startswith('/') and not Path(dep).name.startswith(('libc.', 'libm.', 'libdl.', 'libpthread.', 'ld-linux')):
                shutil.copy(dep, native / Path(dep).name)
    # Copyright/license text is available as embedded POD even for package-manager installs.
    notices = ROOT / 'docs/THIRD_PARTY.md'
    shutil.copy(notices, DEST / 'THIRD_PARTY.md')
    for name in ('Artistic','Copying'):
        candidates = [Path(privlib)/name, Path('/usr/share/common-licenses')/('Artistic' if name=='Artistic' else 'GPL-1')]
        source = next((p for p in candidates if p.is_file()), None)
        if source: shutil.copy(source, DEST / name)
    for name in ('Artistic','Copying'):
        if not (DEST/name).exists():
            checked_in=ROOT/'docs/licenses'/name
            if checked_in.exists(): shutil.copy(checked_in,DEST/name)
            else: raise SystemExit(f'Missing license text: docs/licenses/{name}')
    for file in DEST.rglob('*'):
        if file.is_file(): file.chmod(file.stat().st_mode | 0o200)
    info = {'exiftool':run('exiftool','-ver'),'perl':run(str(perl),'-e','print $^V'), 'platform':platform.system(),'architecture':platform.machine()}
    (DEST/'versions.json').write_text(json.dumps(info,indent=2)+'\n')
    env = dict(os.environ, PERL5LIB=os.pathsep.join(str(DEST/p) for p in ('lib','perl-lib','perl-arch')), LD_LIBRARY_PATH=str(native))
    subprocess.run([str(DEST/'perl'),str(DEST/'exiftool'),'-config','','-ver'],env=env,check=True)
    print(f'Bundled metadata runtime at {DEST}')

if __name__=='__main__': main()
