#!/usr/bin/env bash
# Opt-in developer/operator helper. No automatic live test or startup hook.
set -euo pipefail
if [[ $# -lt 1 ]]; then
  printf '%s\n' 'Read scripts/product_provider_probe.rs for usage and the exact consent boundary.' 'Pass probe, prepare-synthetic, or submit-synthetic explicitly. Only submit-synthetic can invoke a model.'
  exit 0
fi
case "$1" in probe|prepare-synthetic|submit-synthetic) ;; *) echo 'Unknown operation; no model invoked' >&2; exit 2 ;; esac
repo="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/gitmanager-provider-probe.XXXXXXXX")"
trap 'rm -rf -- "$work"' EXIT
cd "$repo"
cargo build --locked -j2 --message-format=json > "$work/build.jsonl"
python3 - "$work/build.jsonl" "$work/externs" <<'PY'
import json, sys
wanted={'serde','serde_json','ring','tempfile'}
found={}
for line in open(sys.argv[1]):
    try: record=json.loads(line)
    except ValueError: continue
    target=record.get('target',{}).get('name')
    if record.get('reason')=='compiler-artifact' and target in wanted:
        for path in record.get('filenames',[]):
            if path.endswith('.rlib'): found[target]=path
if found.keys()!=wanted: raise SystemExit('Missing locked Rust library artifacts')
with open(sys.argv[2],'wb') as output:
    for name,path in sorted(found.items()):
        for arg in ('--extern',name+'='+path,'-L','dependency='+str(__import__('pathlib').Path(path).parent)):
            output.write(arg.encode()+b'\0')
PY
externs=()
while IFS= read -r -d '' argument; do externs+=("$argument"); done < "$work/externs"
rustc --edition 2021 -C debuginfo=0 "${externs[@]}" scripts/product_provider_probe.rs -o "$work/provider-probe"
"$work/provider-probe" "$@"
