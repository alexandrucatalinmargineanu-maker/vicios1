# ViciOS repository layout

The public repository source tree and the binary package repository can live in separate GitHub repositories. For development, this directory documents the latter:

```text
index.json
index.json.sig
packages/*.vpk
```

`tools/vpk.py` creates packages and signs the index. Keep the Ed25519 private key offline. Put only the public key in the installed `/etc/vos/repos.json` file.
