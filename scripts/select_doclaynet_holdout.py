#!/usr/bin/env python3
"""Freeze a metadata-only DocLayNet test sample before any parser execution."""
from pathlib import Path
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
import hashlib,json,datetime,sys
from doclaynet_sample import Archive
ROOT=Path(__file__).resolve().parents[1]
DATA=ROOT/'target/independent-pdf-v2'
SPEC=ROOT/'benchmarks/independent-pdf-v2'
SEED='bulkparse-doclaynet-test-v2-2026-11-11'
def sha(b):return hashlib.sha256(b).hexdigest()
def main():
    raw=json.loads((DATA/'test.json').read_text())
    prior=json.loads((SPEC/'prior-pdf-inventory.json').read_text())
    known_hashes={r['sha256'] for r in prior};known_names={Path(r['path']).name.casefold() for r in prior}
    groups=defaultdict(list);excluded=[]
    for r in raw['images']:
        if r.get('precedence',0)!=0:continue
        if r['doc_name'].casefold() in known_names:
            excluded.append({'image_id':r['id'],'doc_name':r['doc_name'],'reason':'original document filename already exists locally'});continue
        groups[r['doc_category']].append(r)
    selected=[]
    for cat,rows in sorted(groups.items()):
        used=set()
        for r in sorted(rows,key=lambda x:sha((SEED+x['file_name']).encode())):
            if r['doc_name'] in used:continue
            used.add(r['doc_name']);selected.append(r)
            if len(used)==10:break
    plan={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'seed':SEED,'dataset':'DocLayNet 1.0.0 official test split','test_json_sha256':sha((DATA/'test.json').read_bytes()),'selection':'Seeded SHA256 of PNG filename; at most 10 pages per category, one page per original document. Exclude all identifiable previously present original document names. Categories with fewer than 10 eligible original documents contribute all; availability amendment made before viewing parser outputs.','excluded_by_name':excluded,'selected':selected,'categories':{c:sum(r['doc_category']==c for r in selected) for c in groups}}
    path=SPEC/'selection.json'
    if path.exists():
        previous=json.loads(path.read_text());assert previous['selected']==selected
        assert previous['test_json_sha256']==plan['test_json_sha256']
    else:path.write_text(json.dumps(plan,indent=2)+'\n')
    print('Frozen selection:',plan['categories'],flush=True)
    arcs={kind:Archive('DocLayNet_'+kind+'.zip',DATA/('range-cache-'+kind)) for kind in ['core','extra']}
    indices={k:json.loads((DATA/(k+'-index.json')).read_text()) for k in arcs}
    for k in arcs:assert arcs[k].identity==indices[k]['archive']
    for folder in ['pdfs','cells','images']:(DATA/folder).mkdir(exist_ok=True)
    def fetch(r):
        stem=Path(r['file_name']).stem; hashes={}
        for kind,member,folder,suffix in [('extra','PDF/'+stem+'.pdf','pdfs','.pdf'),('extra','JSON/'+stem+'.json','cells','.json'),('core','PNG/'+stem+'.png','images','.png')]:
            dest=DATA/folder/(stem+suffix)
            data=arcs[kind].extract(indices[kind]['members'][member])
            if dest.exists():assert dest.read_bytes()==data
            else:dest.write_bytes(data)
            hashes[folder]=sha(data)
        if hashes['pdfs'] in known_hashes:
            return {'image':r,'excluded':'byte-identical prior PDF','sha256':hashes}
        return {'image':r,'sha256':hashes,'paths':{folder:str((DATA/folder/(stem+suffix)).relative_to(ROOT)) for folder,suffix in [('pdfs','.pdf'),('cells','.json'),('images','.png')]}}
    with ThreadPoolExecutor(max_workers=6) as pool:rows=list(pool.map(fetch,selected))
    manifest={'archives':{k:v.identity for k,v in arcs.items()},'rows':rows,'selection_sha256':sha(path.read_bytes()),'prior_inventory_sha256':sha((SPEC/'prior-pdf-inventory.json').read_bytes()),'policy':'No replacements for overlap/failure exclusions after frozen metadata selection; all recorded.'}
    manifest_path=SPEC/'manifest.json'
    if manifest_path.exists():assert json.loads(manifest_path.read_text())==manifest, 'Restored sample differs from frozen hashes'
    else:manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
    print('Downloaded',len(rows),'selected pages; byte-identical exclusions',sum('excluded' in r for r in rows),flush=True)
if __name__=='__main__':main()
