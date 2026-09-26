#!/usr/bin/env python3
"""Check ELF interpreter and DT_NEEDED against a staged ViciOS root; never run ldd.
Fails if a required library is missing. This is not an ABI or desktop functionality test.
"""
import argparse, os, pathlib, re, subprocess, sys

def elf_info(path):
    with path.open('rb') as f:
        if f.read(4)!=b'\x7fELF': return None
    result=subprocess.run(['readelf','-lWd',str(path)],capture_output=True,text=True,check=True).stdout
    needed=re.findall(r'\(NEEDED\).*?\[(.*?)\]',result)
    interp=re.search(r'Requesting program interpreter: (.*?)\]',result)
    rpaths=re.findall(r'\((?:RUNPATH|RPATH)\).*?\[(.*?)\]',result)
    return needed, interp.group(1) if interp else None, ':'.join(rpaths).split(':') if rpaths else []

def resolve(root,path):
    """Resolve absolute links relative to target, and reject target escapes."""
    root=root.resolve(); path=pathlib.Path(path)
    for _ in range(40):
        if not path.is_relative_to(root): return None
        parts=path.relative_to(root).parts; current=root
        for i,part in enumerate(parts):
            current=current/part
            if current.is_symlink():
                target=os.readlink(current)
                replacement=root/target.lstrip('/') if target.startswith('/') else current.parent/target
                path=pathlib.Path(os.path.normpath(str(replacement.joinpath(*parts[i+1:])))); break
        else: return path if path.is_file() else None
    return None

def audit(root):
    root=pathlib.Path(root).resolve(); errors=[]; count=0
    defaults=['usr/lib','usr/lib64','lib','lib64']
    for path in root.rglob('*'):
        if path.is_symlink() or not path.is_file(): continue
        info=elf_info(path)
        if info is None: continue
        count+=1; needed,interp,rpaths=info
        if interp and not resolve(root,root/interp.lstrip('/')): errors.append(f'{path.relative_to(root)}: missing interpreter {interp}')
        dirs=[]
        for rpath in rpaths:
            rpath=rpath.replace('${ORIGIN}',str(path.parent)).replace('$ORIGIN',str(path.parent))
            if '$' in rpath: errors.append(f'{path.relative_to(root)}: unsupported loader variable {rpath}'); continue
            p=pathlib.Path(rpath)
            if p.is_absolute(): dirs.append(p if p.is_relative_to(root) else root/rpath.lstrip('/'))
        dirs += [root/d for d in defaults]
        for name in needed:
            if '/' in name or not any(resolve(root,d/name) for d in dirs): errors.append(f'{path.relative_to(root)}: missing DT_NEEDED {name}')
    return count,errors
if __name__=='__main__':
    p=argparse.ArgumentParser(); p.add_argument('root'); args=p.parse_args(); count,errors=audit(args.root)
    print(f'{count} ELF files, {len(errors)} unresolved requirements')
    for e in errors: print(e)
    sys.exit(bool(errors))
