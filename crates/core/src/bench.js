'use strict';
const video = document.querySelector('#video');
const select = document.querySelector('#asset');
const status = document.querySelector('#status');
const log = document.querySelector('#log');
const audio = document.querySelector('#audio');
const audioStatus = document.querySelector('#audio-status');
const subtitles = document.querySelector('#subtitles');
const subtitleStatus = document.querySelector('#subtitle-status');
function updateSubtitles() {
  subtitles.replaceChildren();
  const off=document.createElement('option');off.value='off';off.textContent='Désactivés';subtitles.append(off);
  const tracks=video.textTracks;
  for(let i=0;i<tracks.length;i++) {
    const option=document.createElement('option');option.value=String(i);
    option.textContent=`${tracks[i].language || 'langue inconnue'} — ${tracks[i].label || `piste ${i+1}`} (${tracks[i].kind})`;
    subtitles.append(option);if(tracks[i].mode==='showing') option.selected=true;
  }
  subtitles.disabled=tracks.length===0;
  subtitleStatus.textContent=tracks.length ? `${tracks.length} piste(s) texte exposée(s). Vérifier séparément les cues et le rendu.` : 'Aucune piste texte native exposée ; les sous-titres détectés dans le fichier ne sont pas déclarés lisibles ici.';
}
subtitles.addEventListener('change',()=>{
  for(let i=0;i<video.textTracks.length;i++) video.textTracks[i].mode=subtitles.value===String(i)?'showing':'disabled';
  log.textContent+=JSON.stringify({event:'subtitle-selection-requested',value:subtitles.value,time:video.currentTime})+'\n';
});
video.addEventListener('loadedmetadata',updateSubtitles);
video.addEventListener('loadeddata',updateSubtitles);
video.addEventListener('emptied',updateSubtitles);
for(const event of ['addtrack','removetrack','change']) video.textTracks.addEventListener(event,updateSubtitles);
const remuxSelect = document.querySelector('#remux-language');
const remuxStatus = document.querySelector('#remux-status');
const loadButton = document.querySelector('#load');
const bench = window.mediaBench = {inventory:[], activeSource:null, activeAsset:null, pendingSwitch:null, lastSwitch:null};
let switching = false;
function updateRemux() {
  remuxSelect.disabled = switching || video.readyState < 1 || !['fr','en'].every(language => bench.inventory.some(a => a.language_remux?.source_asset === bench.activeSource && a.language_remux.language === language));
}
video.addEventListener('loadedmetadata', updateRemux);
video.addEventListener('loadeddata', updateRemux);
video.addEventListener('emptied', updateRemux);
function waitMedia(event) {
  let finish;
  const promise = new Promise((resolve,reject) => {
    finish = error => { clearTimeout(timer); video.removeEventListener(event,success); video.removeEventListener('error',failure); error ? reject(error) : resolve(); };
    const success = () => finish();
    const failure = () => finish(new Error(`MediaError ${video.error?.code}: ${video.error?.message}`));
    const timer = setTimeout(() => finish(new Error(`Délai dépassé : ${event}`)), 10000);
    video.addEventListener(event,success,{once:true}); video.addEventListener('error',failure,{once:true});
  });
  return {promise, cancel:() => finish()};
}
async function switchLanguage(language) {
  const target = bench.inventory.find(a => a.language_remux?.source_asset === bench.activeSource && a.language_remux.language === language);
  if (switching || !target) return {ok:false, error:'Remux indisponible ou changement déjà en cours'};
  switching = true; updateRemux(); select.disabled = true; loadButton.disabled = true;
  const started = performance.now();
  const record = {method:'prepared-remux-source-reload', fromAsset:bench.activeAsset, toAsset:target.id, language,
    requestedPosition:video.currentTime, wasPlaying:!video.paused && !video.ended, ok:false};
  const previousLanguage = bench.inventory.find(a => a.id===bench.activeAsset)?.language_remux?.language || 'fr';
  let waiter;
  try {
    waiter = waitMedia('loadedmetadata');
    video.src = `/api/v1/assets/${encodeURIComponent(target.id)}/content`; video.load();
    await waiter.promise;
    record.metadataMs = performance.now()-started;
    if (!Number.isFinite(video.duration) || record.requestedPosition >= video.duration) throw new Error('Position hors de la durée du remux');
    if (record.requestedPosition > 0) {
      waiter = waitMedia('seeked'); video.currentTime = record.requestedPosition; await waiter.promise;
    }
    record.restoredPosition = video.currentTime;
    record.positionErrorSeconds = Math.abs(record.restoredPosition-record.requestedPosition);
    if (record.positionErrorSeconds > 0.15) throw new Error('Position non restaurée à 150 ms près');
    record.seekReadyMs = performance.now()-started;
    if (record.wasPlaying) await video.play();
    bench.activeAsset = target.id; record.ok = true;
    remuxStatus.textContent = `Source ${language.toUpperCase()} chargée ; position restaurée à ${record.restoredPosition.toFixed(3)} s. Changement avec interruption possible, son à vérifier.`;
  } catch(error) {
    record.error = error.message; remuxSelect.value = previousLanguage;
    remuxStatus.textContent = `Échec du changement : ${error.message}. Recharger une source pour reprendre.`;
    // The old source is not restored automatically; avoid claiming it is still playing.
    bench.activeAsset = null;
  } finally {
    waiter?.cancel(); record.completedMs = performance.now()-started;
    bench.lastSwitch = record;
    log.textContent += JSON.stringify({event:'remux-source-switch', ...record})+'\n';
    switching = false; updateRemux(); select.disabled = false; loadButton.disabled = false;
  }
  return record;
}
remuxSelect.addEventListener('change', () => { bench.pendingSwitch = switchLanguage(remuxSelect.value); });
function updateAudio() {
  audio.replaceChildren();
  const tracks = video.audioTracks;
  audio.disabled = !tracks || tracks.length < 2;
  if (!tracks) {
    audioStatus.textContent = 'Sélection native indisponible : ce navigateur n’expose pas HTMLMediaElement.audioTracks avec ses réglages actuels.';
    return;
  }
  for (let i=0; i<tracks.length; i++) {
    const option = document.createElement('option');
    option.value = String(i);
    option.textContent = `${tracks[i].language || 'langue inconnue'} — ${tracks[i].label || `piste ${i+1}`}`;
    option.selected = tracks[i].enabled;
    audio.append(option);
  }
  audioStatus.textContent = `${tracks.length} piste(s) audio exposée(s). Le changement doit être confirmé à l’écoute ou par mesure du signal.`;
}
video.addEventListener('loadedmetadata', updateAudio);
video.addEventListener('emptied', () => { audio.disabled = true; audio.replaceChildren(); audioStatus.textContent = 'Source en chargement.'; });
if (video.audioTracks) {
  for (const event of ['addtrack','removetrack']) video.audioTracks.addEventListener(event, updateAudio);
}
audio.addEventListener('change', () => {
  const tracks = video.audioTracks;
  const index = Number(audio.value);
  if (!tracks || !Number.isInteger(index) || index<0 || index>=tracks.length) return;
  for (let i=0; i<tracks.length; i++) tracks[i].enabled = i===index;
  log.textContent += JSON.stringify({event:'audio-selection-requested', index, language:tracks[index].language, time:video.currentTime}) + '\n';
});
for (const event of ['loadedmetadata', 'playing', 'pause', 'seeking', 'seeked', 'ended', 'error']) {
  video.addEventListener(event, () => {
    const record = {event, time: video.currentTime, readyState: video.readyState,
      error: video.error && {code: video.error.code, message: video.error.message}};
    log.textContent += JSON.stringify(record) + '\n';
    status.textContent = event === 'error' ? `Échec navigateur : ${video.error?.code} ${video.error?.message}` : event;
  });
}
document.querySelector('#load').addEventListener('click', () => {
  log.textContent = '';
  bench.activeSource = select.value; bench.activeAsset = select.value;
  remuxSelect.value = 'fr'; bench.lastSwitch = null; updateRemux();
  remuxStatus.textContent = 'Source d’origine en chargement. Pour MKV/MP4 préparés, le choix de langue remplacera le fichier et restaurera la position.';
  video.src = `/api/v1/assets/${encodeURIComponent(select.value)}/content`;
  video.load();
});
fetch('/api/v1/assets').then(async response => {
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  bench.inventory = await response.json();
  for (const asset of bench.inventory.filter(a => !a.language_remux)) {
    const option = document.createElement('option');
    option.value = asset.id;
    option.textContent = `${asset.id} (${asset.probe.streams.map(t => t.codec_name).join(', ')})`;
    select.append(option);
  }
  status.textContent = 'Choisir une source, charger puis lancer la lecture.';
}).catch(error => { status.textContent = `Inventaire indisponible : ${error.message}`; });
