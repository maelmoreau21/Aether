import { self } from '$app/service-worker';
import { immutable } from '$app/manifest';
import { version } from '$app/env';
import { CHUNK_SIZE, offlineItem, offlineChunk, unlockDetails } from '../lib/offline-vault';
const CACHE=`media-shell-${version}`;
const SHELL=['/app/downloads/','/app/downloads/index-bootstrap-0.js',...immutable.map(asset=>'/app/'+asset.path.replace(/^\//,''))];
const ASSETS=new Set(SHELL);
self.addEventListener('install',event=>{event.waitUntil(caches.open(CACHE).then(cache=>cache.addAll(SHELL)));});
self.addEventListener('activate',event=>{event.waitUntil(caches.keys().then(keys=>Promise.all(keys.filter(key=>key.startsWith('media-shell-')&&key!==CACHE).map(key=>caches.delete(key)))));});
async function keyFromClient(id:string,clientId:string):Promise<CryptoKey|undefined>{
 const client=await self.clients.get(clientId);if(!client)return;
 return new Promise(resolve=>{const channel=new MessageChannel();const timeout=setTimeout(()=>{channel.port1.close();resolve(undefined);},3000);channel.port1.onmessage=event=>{clearTimeout(timeout);channel.port1.close();resolve(event.data instanceof CryptoKey?event.data:undefined);};client.postMessage({type:'offline-key',id},[channel.port2]);});
}
async function localMedia(request:Request,id:string,clientId:string):Promise<Response>{
 const item=await offlineItem(id);if(!item?.complete)return new Response('Téléchargement absent ou incomplet',{status:404});
 const key=await keyFromClient(id,clientId);if(!key)return new Response('Déverrouillez ce téléchargement',{status:403});
 let metadata;try{metadata=await unlockDetails(item,key);}catch{return new Response('Clé incorrecte',{status:403});}
 let start=0;let end=item.size-1;const range=request.headers.get('range');
 if(range){const match=/^bytes=(\d*)-(\d*)$/.exec(range);if(!match||!match[1]&&!match[2])return new Response(null,{status:416,headers:{'Content-Range':`bytes */${item.size}`}});if(match[1]){start=Number(match[1]);end=match[2]?Math.min(Number(match[2]),end):end;}else{start=Math.max(0,item.size-Number(match[2]));}if(!Number.isSafeInteger(start)||!Number.isSafeInteger(end)||start>end||start>=item.size||Number(match[2])===0&&!match[1])return new Response(null,{status:416,headers:{'Content-Range':`bytes */${item.size}`}});}
 const headers=new Headers({'Content-Type':metadata.mime,'Content-Length':String(end-start+1),'Accept-Ranges':'bytes','Cache-Control':'no-store','X-Content-Type-Options':'nosniff'});if(range)headers.set('Content-Range',`bytes ${start}-${end}/${item.size}`);
 let index=Math.floor(start/CHUNK_SIZE);const last=Math.floor(end/CHUNK_SIZE);
 const body=request.method==='HEAD'?null:new ReadableStream<Uint8Array>({async pull(controller){try{if(index>last){controller.close();return;}const chunk=await offlineChunk(id,index);if(!chunk)throw new Error('Bloc manquant');const decoded=new Uint8Array(await crypto.subtle.decrypt({name:'AES-GCM',iv:Uint8Array.from(chunk.iv)},key,chunk.data));const from=index===Math.floor(start/CHUNK_SIZE)?start%CHUNK_SIZE:0;const to=index===last?end%CHUNK_SIZE+1:decoded.length;controller.enqueue(decoded.subarray(from,to));index++;}catch(error){controller.error(error);}}});
 return new Response(body,{status:range?206:200,headers});
}
self.addEventListener('fetch',event=>{
 const url=new URL(event.request.url);if(url.origin!==self.location.origin)return;
 if(url.pathname.startsWith('/app/offline-media/')&&['GET','HEAD'].includes(event.request.method)){event.respondWith(localMedia(event.request,url.pathname.split('/').pop()??'',event.clientId));return;}
 if(event.request.method!=='GET')return;
 if(ASSETS.has(url.pathname)){event.respondWith((async()=>{const cache=await caches.open(CACHE);return await cache.match(url.pathname)??fetch(event.request);})());return;}
 if(event.request.mode!=='navigate'||!url.pathname.startsWith('/app/'))return;
 event.respondWith(fetch(event.request).catch(()=>new Response('<!doctype html><html lang="fr"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Connexion indisponible</title><h1>Connexion indisponible</h1><p>Vérifiez votre connexion et la disponibilité du serveur.</p><a href="/app/downloads/">Mes téléchargements hors ligne</a><p><a href="/app/">Réessayer</a></p></html>',{status:503,headers:{'Content-Type':'text/html; charset=utf-8','Cache-Control':'no-store'}})));
});
