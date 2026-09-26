#!/usr/bin/env python3
"""Pack staged files, sign a repository index, or generate a private development key."""
import argparse, gzip, hashlib, io, json, os, pathlib, subprocess, tarfile, tempfile, time
ABI = 'x86_64-vicios-gnu'
def run(*args):
    return subprocess.run(args, check=True, stdout=subprocess.PIPE).stdout

def pack(manifest, tree, output):
    tree, output = pathlib.Path(tree), pathlib.Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    data = json.dumps(manifest, sort_keys=True).encode()
    with output.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0, filename='') as gz, tarfile.open(fileobj=gz, mode='w') as tar:
        info = tarfile.TarInfo('manifest.json'); info.size = len(data); info.mode = 0o644
        tar.addfile(info, io.BytesIO(data))
        for path in sorted(tree.rglob('*')):
            if path.is_dir() and not path.is_symlink(): continue
            rel = path.relative_to(tree).as_posix()
            if rel.split('/')[0] not in ('usr', 'etc', 'opt', 'boot'): raise ValueError(f'unsupported payload path: {rel}')
            info = tar.gettarinfo(str(path), arcname='files/' + rel)
            info.uid = info.gid = info.mtime = 0; info.uname = info.gname = ''
            if info.isfile():
                with path.open('rb') as f: tar.addfile(info, f)
            elif info.issym(): tar.addfile(info)
            else: raise ValueError(f'unsupported payload type: {rel}')
    return {'manifest': manifest, 'file': output.name, 'sha256': hashlib.sha256(output.read_bytes()).hexdigest(), 'size': output.stat().st_size}

def keygen(path):
    path = pathlib.Path(path)
    if path.exists(): raise ValueError('refusing to overwrite a key')
    run('openssl', 'genpkey', '-algorithm', 'ED25519', '-out', str(path)); path.chmod(0o600)
    return public_key(path)

def public_key(path):
    der = run('openssl', 'pkey', '-in', str(path), '-pubout', '-outform', 'DER')
    if der[:12].hex() != '302a300506032b6570032100': raise ValueError('not an Ed25519 key')
    return der[12:].hex()

def publish(repo, key, records, serial, expires=None):
    repo = pathlib.Path(repo); repo.mkdir(parents=True, exist_ok=True)
    idx = {'format': 2, 'serial': serial, 'expires': expires or int(time.time()) + 7*86400, 'abi': ABI, 'packages': records}
    raw = json.dumps(idx, sort_keys=True, indent=2).encode() + b'\n'
    # Build and sign locally; publish archives first, index + signature as one release.
    with tempfile.TemporaryDirectory() as td:
        f = pathlib.Path(td)/'index'; f.write_bytes(raw)
        sig = run('openssl','pkeyutl','-sign','-rawin','-inkey',str(key),'-in',str(f))
    (repo/'index.json').write_bytes(raw); (repo/'index.json.sig').write_text(sig.hex()+'\n')
    return idx

def main():
    p=argparse.ArgumentParser(); sub=p.add_subparsers(dest='cmd',required=True)
    k=sub.add_parser('keygen'); k.add_argument('key')
    a=sub.add_parser('pack'); a.add_argument('manifest'); a.add_argument('tree'); a.add_argument('output')
    a=sub.add_parser('index'); a.add_argument('repo'); a.add_argument('key'); a.add_argument('--serial',type=int,required=True)
    args=p.parse_args()
    if args.cmd=='keygen': print(keygen(args.key))
    elif args.cmd=='pack':
        record=pack(json.loads(pathlib.Path(args.manifest).read_text()),args.tree,args.output)
        pathlib.Path(args.output+'.json').write_text(json.dumps(record,indent=2)+'\n')
    else:
        records=[json.loads(f.read_text()) for f in sorted(pathlib.Path(args.repo,'packages').glob('*.vpk.json'))]
        if len({r['manifest']['name'] for r in records}) != len(records): raise ValueError('duplicate package names; keep one release per name')
        publish(args.repo,args.key,records,args.serial)
if __name__=='__main__': main()
