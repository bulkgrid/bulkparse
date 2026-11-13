import {init, parse} from '../index.js';
await init();
postMessage({ready:true});
onmessage = ({data}) => {
  try { postMessage({result:parse(new Uint8Array(data.buffer), {filename:data.filename})}); }
  catch (error) {postMessage({error:`${error.code ?? 'error'}: ${error.message}`});}
};
