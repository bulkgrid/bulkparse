// Run inside the built service image with this checkout mounted read-only at /data.
import assert from 'node:assert/strict';
import { fork, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
assert.equal(spawnSync('python3', ['--version']).error?.code, 'ENOENT', 'runtime must not require Python');
const env = { ...process.env, SUPABASE_URL: 'http://127.0.0.1:1', SUPABASE_API_KEY: 'test-key', GCP_PROJECT_ID: 'validation', GCS_BUCKET_PREFIX: 'validation', ENVIRONMENT: 'validation' };
const server = fork('/app/apps/fileConverter/dist/index.js', [], { env, stdio: ['ignore', 'inherit', 'inherit', 'ipc'] });
try {
  let health;
  for (let attempt=0; attempt<50; attempt++) {
    try { health=await fetch('http://127.0.0.1:8084/health'); if (health.ok) break; } catch {}
    await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert.deepEqual(await health?.json(), { status:'ok', service:'fileConverter' });
} finally { const stopped=once(server,'exit'); server.kill('SIGTERM'); await stopped; }
const child=fork('/app/apps/fileConverter/dist/parserWorker.js',[],{stdio:['ignore','inherit','inherit','ipc'],env});
const directory=await mkdtemp(path.join(tmpdir(),'native-container-'));
const results=[];
try {
  for (const [index,name] of ['table.jpg','table.pdf','two-frames.tiff'].entries()) {
    const id=String(index),input=`/data/target/native-ocr/bindings-normalized/${name}`,output=path.join(directory,`${id}.md`);
    const response=await new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>reject(new Error('native parser timeout')),120000);
      child.once('message',message=>{clearTimeout(timer);resolve(message);});
      child.send({id,input,output});
    });
    assert.equal(response.ok,true,JSON.stringify(response));
    const markdown=await readFile(output,'utf8');
    for(const value of ['Sales report','Product','Amount','Apples','125'])assert.ok(markdown.includes(value),`${name}: missing ${value}`);
    assert.equal(response.pageCount,name==='two-frames.tiff'?2:1);
    results.push({name,pageCount:response.pageCount,nonempty:true});
  }
} finally { const stopped=once(child,'exit');child.kill('SIGKILL');await stopped;await rm(directory,{recursive:true,force:true}); }
console.log(JSON.stringify({platform:process.platform,architecture:process.arch,pythonInstalled:false,health:true,results}));
