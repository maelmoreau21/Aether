import { invalidateCatalog } from './page-cache';
import { get, writable } from 'svelte/store';
import { getSessionGeneration, onSessionGenerationChange } from '../auth/session';
export interface AccessibleLibrary { id:string;name:string;kind:'films'|'series'|'music' }
export const accessibleLibraries=writable<AccessibleLibrary[]>([]);
let pending:Promise<void>|undefined;
let loadedAt=0;
onSessionGenerationChange(()=>{accessibleLibraries.set([]);pending=undefined;loadedAt=0;});
export async function loadLibraries(force=false):Promise<void> {
  if(pending)return pending;
  if(!force&&Date.now()-loadedAt<30000)return;
  const generation=getSessionGeneration();
  const current=(async()=>{
    const response=await fetch('/api/v1/libraries',{cache:'no-store',signal:AbortSignal.timeout(8000)});
    if(!response.ok)throw new Error('Bibliothèques indisponibles.');
    const data=await response.json();
    if(generation===getSessionGeneration()){
      const previous = get(accessibleLibraries);
      const next = data.libraries as AccessibleLibrary[];
      for (const library of previous) {
        if (!next.some(item => item.id === library.id && item.name === library.name && item.kind === library.kind)) invalidateCatalog(library.id);
      }
      accessibleLibraries.set(next);loadedAt=Date.now();
    }
  })();
  pending=current;
  try{await current;}finally{if(pending===current)pending=undefined;}
}
