// Encrypted, chunked local media. Device keys are non-extractable CryptoKeys;
// legacy passphrase downloads remain readable without rewriting their data.
import { downloadUrl } from './download-url';
export const CHUNK_SIZE=1024*1024;
export interface OfflineItem { id:string; salt:Uint8Array; size:number; chunks:number; complete:boolean; metadata?:{iv:Uint8Array;data:ArrayBuffer}; createdAt:number; deviceKey?:boolean }
export interface OfflineDetails { title:string; mime:string; size:number }
export interface EncryptedChunk { iv:Uint8Array; data:ArrayBuffer }
let database:Promise<IDBDatabase>|undefined;
let deviceDatabase:Promise<IDBDatabase>|undefined;
function open(name:string,upgrade:(database:IDBDatabase)=>void):Promise<IDBDatabase>{return new Promise((resolve,reject)=>{const request=indexedDB.open(name,1);request.onupgradeneeded=()=>upgrade(request.result);request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);});}
function db(){return database??=open('media-offline-v1',database=>{database.createObjectStore('items',{keyPath:'id'});database.createObjectStore('chunks');});}
function keyDb(){return deviceDatabase??=open('media-offline-device-keys-v1',database=>database.createObjectStore('keys'));}
async function access<T>(store:string,mode:IDBTransactionMode,action:(store:IDBObjectStore)=>IDBRequest<T>):Promise<T>{const database=await(store==='keys'?keyDb():db());return new Promise((resolve,reject)=>{const transaction=database.transaction(store,mode);const request=action(transaction.objectStore(store));transaction.oncomplete=()=>resolve(request.result);transaction.onerror=()=>reject(transaction.error);transaction.onabort=()=>reject(transaction.error);});}
export async function offlineItems():Promise<OfflineItem[]>{return access('items','readonly',store=>store.getAll());}
export async function offlineItem(id:string):Promise<OfflineItem|undefined>{return access('items','readonly',store=>store.get(id));}
export async function offlineChunk(id:string,index:number):Promise<EncryptedChunk|undefined>{return access('chunks','readonly',store=>store.get([id,index]));}
export async function removeOffline(id:string){await access('chunks','readwrite',store=>store.delete(IDBKeyRange.bound([id,0],[id,Number.MAX_SAFE_INTEGER])));await access('items','readwrite',store=>store.delete(id));await access('keys','readwrite',store=>store.delete(id));}
export async function storedOfflineKey(id:string):Promise<CryptoKey|undefined>{return access('keys','readonly',store=>store.get(id));}
export async function offlineKey(passphrase:string,salt:Uint8Array):Promise<CryptoKey>{const material=await crypto.subtle.importKey('raw',new TextEncoder().encode(passphrase),'PBKDF2',false,['deriveKey']);return crypto.subtle.deriveKey({name:'PBKDF2',salt:Uint8Array.from(salt),iterations:310000,hash:'SHA-256'},material,{name:'AES-GCM',length:256},false,['encrypt','decrypt']);}
async function encrypt(key:CryptoKey,bytes:Uint8Array):Promise<EncryptedChunk>{const iv=crypto.getRandomValues(new Uint8Array(12));return{iv,data:await crypto.subtle.encrypt({name:'AES-GCM',iv},key,Uint8Array.from(bytes))};}
export async function unlockDetails(item:OfflineItem,key:CryptoKey):Promise<OfflineDetails>{if(!item.metadata)throw new Error('Téléchargement incomplet.');const decoded=await crypto.subtle.decrypt({name:'AES-GCM',iv:Uint8Array.from(item.metadata.iv)},key,item.metadata.data);return JSON.parse(new TextDecoder().decode(decoded));}
export async function saveOffline(url:string,title:string,mime:string,passphrase:string|null,signal:AbortSignal,progress:(bytes:number,total:number)=>void):Promise<void>{
 if(passphrase!==null&&passphrase.length<12)throw new Error('Choisissez une phrase de déverrouillage d’au moins 12 caractères.');
 if(!isSecureContext||!crypto.subtle)throw new Error('Le téléchargement hors ligne nécessite HTTPS ou localhost.');
 const source=downloadUrl(url,mime);if(!source)throw new Error('Source de téléchargement indisponible.');
 const response=await fetch(source,{signal,credentials:'same-origin',cache:'no-store'});
 if(!response.ok||!response.body)throw new Error(`Téléchargement refusé (HTTP ${response.status}).`);
 const actualMime=response.headers.get('content-type')?.split(';')[0]??mime;
 if(!/^(audio|video)\/[a-z0-9.+-]+$/i.test(actualMime)){await response.body.cancel();throw new Error('La réponse reçue n’est pas un média.');}
 const size=Number(response.headers.get('content-length'));
 if(!Number.isSafeInteger(size)||size<=0||size>4*1024**3){await response.body.cancel();throw new Error('La taille doit être connue et inférieure à 4 Gio par téléchargement.');}
 const estimate=await navigator.storage.estimate();if((estimate.quota??0)-(estimate.usage??0)<size*1.1){await response.body.cancel();throw new Error('Espace de stockage navigateur insuffisant.');}
 const id=crypto.randomUUID();const salt=crypto.getRandomValues(new Uint8Array(16));const key=passphrase===null?await crypto.subtle.generateKey({name:'AES-GCM',length:256},false,['encrypt','decrypt']):await offlineKey(passphrase,salt);
 const item:OfflineItem={id,salt,size,chunks:0,complete:false,createdAt:Date.now(),deviceKey:passphrase===null};await access('items','readwrite',store=>store.put(item));
 const reader=response.body.getReader();let buffer=new Uint8Array(CHUNK_SIZE);let used=0;let received=0;
 try{
  if(item.deviceKey)await access('keys','readwrite',store=>store.put(key,id));
  while(true){signal.throwIfAborted();const result=await reader.read();if(result.done)break;let offset=0;received+=result.value.length;if(received>size)throw new Error('Taille reçue différente de la taille annoncée.');
   while(offset<result.value.length){const take=Math.min(CHUNK_SIZE-used,result.value.length-offset);buffer.set(result.value.subarray(offset,offset+take),used);used+=take;offset+=take;if(used===CHUNK_SIZE){const chunk=await encrypt(key,buffer);await access('chunks','readwrite',store=>store.put(chunk,[id,item.chunks++]));buffer=new Uint8Array(CHUNK_SIZE);used=0;}}
   progress(received,size);
  }
  if(received!==size)throw new Error('Téléchargement incomplet.');if(used){const chunk=await encrypt(key,buffer.subarray(0,used));await access('chunks','readwrite',store=>store.put(chunk,[id,item.chunks++]));}
  item.metadata=await encrypt(key,new TextEncoder().encode(JSON.stringify({title,mime:actualMime,size})));item.complete=true;await access('items','readwrite',store=>store.put(item));
 }catch(error){await reader.cancel().catch(()=>{});await removeOffline(id);throw error;}finally{reader.releaseLock();}
}
