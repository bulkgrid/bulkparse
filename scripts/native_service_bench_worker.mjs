import {fork} from 'node:child_process';
import {createInterface} from 'node:readline';
import {mkdtemp,readFile,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
const service=process.env.BULKPARSE_SERVICE_ROOT;
if(!service || !process.env.BULKPARSE_OCR_CONFIG)throw new Error('Set BULKPARSE_SERVICE_ROOT and BULKPARSE_OCR_CONFIG');
const child=fork(path.join(service,'apps/fileConverter/dist/parserWorker.js'),[],{stdio:['ignore','ignore','inherit','ipc'],env:{PATH:process.env.PATH,RAYON_NUM_THREADS:'1',OMP_NUM_THREADS:'1',OPENBLAS_NUM_THREADS:'1',BULKPARSE_OCR_CONFIG:process.env.BULKPARSE_OCR_CONFIG}});
console.log(JSON.stringify({ready:true,children:[child.pid]}));
try {
 for await(const line of createInterface({input:process.stdin})) {
  const request=JSON.parse(line),directory=await mkdtemp(path.join(tmpdir(),'native-perf-'));
  const input=path.join(directory,'input'+path.extname(request.path)),output=path.join(directory,'output.md');
  const start=performance.now();
  try {
   await writeFile(input,await readFile(request.path));
   const result=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{child.kill('SIGKILL');reject(new Error('timeout'));},120000);child.once('message',v=>{clearTimeout(timer);resolve(v)});child.send({id:request.id,input,output});});
   if(!result.ok)throw new Error(result.message);
   const markdown=await readFile(output);
   console.log(JSON.stringify({id:request.id,seconds:(performance.now()-start)/1000,bytes:markdown.length,sha256:createHash('sha256').update(markdown).digest('hex')}));
  }catch(error){console.log(JSON.stringify({id:request.id,error:String(error)}));}
  finally {await rm(directory,{recursive:true,force:true});}
 }
}finally{child.kill('SIGKILL');}
