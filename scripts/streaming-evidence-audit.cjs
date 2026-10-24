// Fail closed before copying public evidence. Never print matched values.
const fs=require('node:fs'),path=require('node:path');
const dir=path.resolve(process.argv[2]||'docs/evidence/2026-10-24/streaming-system');
const patterns=[/[?&]token=(?!\[redacted\])[^\s"'&]+/i,/media_session=(?!\[redacted\])[a-z0-9._-]+/i,/"(?:csrfToken|bootstrap_token|pairingToken|password)"\s*:\s*"(?!\[redacted\])[^"\s]+"/i,/(?:x-csrf-token|authorization):\s*(?!\[redacted\])[a-z0-9._-]{16}/i];
const flagged=[];let count=0;
function visit(current){for(const ent of fs.readdirSync(current,{withFileTypes:true})){const file=path.join(current,ent.name);if(ent.isDirectory()){if(ent.name==='private-runtime')flagged.push(path.relative(dir,file));else visit(file);}else if(ent.isFile()){count++;if(/\.(?:db|sqlite|sqlite3)(?:-wal|-shm)?$|bootstrap_token|node_identity\.json/i.test(ent.name)){flagged.push(path.relative(dir,file));continue;}if(/\.(?:json|log|md|patch)$/.test(ent.name)){const text=fs.readFileSync(file,'utf8');if(patterns.some(pattern=>pattern.test(text)))flagged.push(path.relative(dir,file));}}}}
visit(dir);console.log(JSON.stringify({filesChecked:count,flaggedFiles:flagged.length,flaggedFileNames:flagged}));if(flagged.length)process.exitCode=1;
