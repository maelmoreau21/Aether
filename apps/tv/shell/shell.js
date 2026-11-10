/*
 * Aether TV shell (Samsung Tizen, LG webOS): asks for the Core address once, then opens the Core's own web
 * interface in TV mode (`/app/?tv=1`) as the page itself. An iframe is not possible: the Core forbids framing
 * (`frame-ancestors 'none'`) and its session cookie is `SameSite=Strict`. Written for the old Chromium of TVs
 * (no modules, no optional chaining).
 */
(function () {
  'use strict';

  var STORAGE_KEY = 'aether.tv.server';
  var AUTO_DELAY_MS = 2500;
  var PROBE_TIMEOUT_MS = 8000;
  var TEXTS = {
    fr: {
      label: 'Adresse de votre serveur Aether',
      hint: 'Le serveur doit être joignable en HTTPS depuis ce téléviseur.',
      connect: 'Se connecter',
      change: 'Changer de serveur',
      checking: 'Recherche du serveur…',
      invalid: 'Adresse invalide : saisissez par exemple https://aether.maison.lan',
      unreachable: 'Aucun serveur Aether ne répond à cette adresse.',
      auto: 'Connexion à {server}… Appuyez sur une touche pour changer de serveur.'
    },
    en: {
      label: 'Your Aether server address',
      hint: 'This TV must reach the server over HTTPS.',
      connect: 'Connect',
      change: 'Change server',
      checking: 'Looking for the server…',
      invalid: 'Invalid address: type for example https://aether.home.lan',
      unreachable: 'No Aether server answers at this address.',
      auto: 'Connecting to {server}… Press any key to change server.'
    }
  };

  /** Origin of the Core from what the user typed, or null. HTTPS unless another scheme is given. */
  function normalizeAddress(raw) {
    var text = String(raw || '').trim();
    if (!text) return null;
    if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) text = 'https://' + text;
    var url;
    try { url = new URL(text); } catch (error) { return null; }
    if ((url.protocol !== 'https:' && url.protocol !== 'http:') || !url.hostname || url.username || url.password) return null;
    return url.protocol + '//' + url.host;
  }

  /** Same key codes as apps/web/src/lib/tv/keys.ts. */
  function remoteKey(event) {
    var code = event.keyCode;
    if (event.key === 'ArrowUp' || code === 38) return 'up';
    if (event.key === 'ArrowDown' || code === 40) return 'down';
    if (event.key === 'ArrowLeft' || code === 37) return 'left';
    if (event.key === 'ArrowRight' || code === 39) return 'right';
    if (code === 10009 || code === 461 || event.key === 'GoBack' || event.key === 'XF86Back') return 'back';
    return null;
  }

  var api = { normalizeAddress: normalizeAddress, remoteKey: remoteKey };
  if (typeof module !== 'undefined' && module.exports) { module.exports = api; return; }
  window.AetherShell = api;

  var language = /^en/i.test(navigator.language || '') ? 'en' : 'fr';
  var texts = TEXTS[language];
  document.documentElement.lang = language;
  var nodes = document.querySelectorAll('[data-text]');
  for (var i = 0; i < nodes.length; i++) nodes[i].textContent = texts[nodes[i].getAttribute('data-text')];

  var form = document.getElementById('connect');
  var input = document.getElementById('address');
  var submit = document.getElementById('submit');
  var change = document.getElementById('change');
  var status = document.getElementById('status');
  var autoTimer = null;
  var busy = false;

  function say(message, error) {
    status.textContent = message || '';
    status.className = error ? 'status error' : 'status';
  }

  function saved() {
    try { return localStorage.getItem(STORAGE_KEY); } catch (error) { return null; }
  }

  function tizenApi() { return window.tizen || null; }

  /** Tizen only delivers the media keys to applications that ask; the Core page asks again. */
  function registerKeys() {
    var tizen = tizenApi();
    if (!tizen || !tizen.tvinputdevice) return;
    var keys = ['MediaPlayPause', 'MediaPlay', 'MediaPause', 'MediaStop', 'MediaFastForward', 'MediaRewind', 'MediaTrackNext', 'MediaTrackPrevious'];
    for (var k = 0; k < keys.length; k++) {
      try { tizen.tvinputdevice.registerKey(keys[k]); } catch (error) { /* Key missing on this model. */ }
    }
  }

  function exitApp() {
    var tizen = tizenApi();
    try {
      if (tizen && tizen.application) { tizen.application.getCurrentApplication().exit(); return; }
    } catch (error) { /* Not a Tizen application. */ }
    window.close();
  }

  /** The Core answers if its web interface serves the brand mark (an image needs no CORS). */
  function probe(origin, done) {
    var image = new Image();
    var finished = false;
    var timer = setTimeout(function () { finish(false); }, PROBE_TIMEOUT_MS);
    function finish(ok) {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      image.onload = image.onerror = null;
      done(ok);
    }
    image.onload = function () { finish(true); };
    image.onerror = function () { finish(false); };
    image.src = origin + '/app/images/brand-mark.svg?tv-shell=' + Date.now();
  }

  function open(origin) {
    try { localStorage.setItem(STORAGE_KEY, origin); } catch (error) { /* Asked again next time. */ }
    registerKeys();
    // replace(): Back on the Core's home screen leaves the application instead of returning here.
    window.location.replace(origin + '/app/?tv=1');
  }

  function connect() {
    cancelAuto();
    if (busy) return;
    var origin = normalizeAddress(input.value);
    if (!origin) { say(texts.invalid, true); input.focus(); return; }
    input.value = origin;
    busy = true;
    say(texts.checking);
    probe(origin, function (ok) {
      busy = false;
      if (ok) open(origin);
      else { say(texts.unreachable, true); input.focus(); }
    });
  }

  function cancelAuto() {
    if (autoTimer === null) return;
    clearTimeout(autoTimer);
    autoTimer = null;
    say('');
  }

  var order = [input, submit, change];
  function visible(element) { return !element.hidden; }
  function move(step) {
    var list = order.filter(visible);
    var index = list.indexOf(document.activeElement);
    var next = list[Math.max(0, Math.min(list.length - 1, index + step))] || list[0];
    next.focus();
  }

  form.addEventListener('submit', function (event) { event.preventDefault(); connect(); });
  change.addEventListener('click', function () { cancelAuto(); input.select(); input.focus(); });
  document.addEventListener('keydown', function (event) {
    if (autoTimer !== null) { event.preventDefault(); cancelAuto(); input.focus(); return; }
    var key = remoteKey(event);
    if (key === 'back') { event.preventDefault(); exitApp(); return; }
    if (key === 'down' || (key === 'right' && document.activeElement !== input)) { event.preventDefault(); move(1); }
    else if (key === 'up' || (key === 'left' && document.activeElement !== input)) { event.preventDefault(); move(-1); }
  });

  registerKeys();
  var previous = saved();
  if (previous) {
    input.value = previous;
    change.hidden = false;
    submit.focus();
    say(texts.auto.replace('{server}', previous));
    autoTimer = setTimeout(function () { autoTimer = null; connect(); }, AUTO_DELAY_MS);
  } else {
    input.focus();
  }
})();
