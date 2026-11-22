"""Make an immutable-by-convention source receipt for isolated validation."""
from pathlib import Path
import datetime,hashlib,json,shutil

root=Path(__file__).resolve().parents[1]
evidence=root/'artifacts/terminal-stability'
baseline=json.loads((evidence/'baseline-provenance.json').read_text(encoding='utf-8-sig'))['integrated']
names=set(baseline)
for directory in ['crates','desktop/src','desktop/src-tauri/src','desktop/tests','desktop/scripts','scripts']:
    for path in (root/directory).rglob('*'):
        if path.is_file() and path.suffix in {'.rs','.toml','.ts','.tsx','.css','.mjs','.py','.ps1','.sh'}:
            names.add(path.relative_to(root).as_posix())
hashes={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in sorted(names)}
digest=hashlib.sha256(''.join(name+'\0'+value+'\n' for name,value in hashes.items()).encode()).hexdigest()
snapshot=evidence/('source-'+digest[:12]);snapshot.mkdir(exist_ok=True)
for name,value in hashes.items():
    dest=snapshot/name
    if dest.exists():assert hashlib.sha256(dest.read_bytes()).hexdigest()==value,name
    else:dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(root/name,dest)
app=evidence/'candidate-app'
artifact_hashes={path.relative_to(app).as_posix():hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(app.rglob('*')) if path.is_file() and path.name!='build-receipt.json'}
receipt={'state':'fixed_for_validation','created':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_root':str(root),'snapshot':str(snapshot),'source_digest':digest,'files':hashes,'artifacts':artifact_hashes,'changed_from_integrated_baseline':[name for name,value in hashes.items() if baseline.get(name)!=value]}
(snapshot/'source-receipt.json').write_text(json.dumps(receipt,indent=2),encoding='utf-8')
(evidence/'candidate-provenance.json').write_text(json.dumps(receipt,indent=2),encoding='utf-8')
print(json.dumps({'snapshot':str(snapshot),'source_digest':digest,'source_files':len(hashes),'changed_files':len(receipt['changed_from_integrated_baseline']),'binaries':{name:value for name,value in artifact_hashes.items() if name.endswith('.exe')}}))
