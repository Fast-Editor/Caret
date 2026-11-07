/*
 * Caret remote shim: makes a plain browser look like the Tauri webview to @tauri-apps/api.
 * Served as /__remote/shim.js and injected as the first script of index.html by `caret serve`.
 * Every `invoke()` becomes a JSON frame on the /ws relay; the desktop executor runs the real command and the reply
 * (or streamed Channel / event callbacks) comes back as `reply` / `cb` frames.
 */
(function () {
  if (typeof window === 'undefined' || window.__TAURI_INTERNALS__) return;

  var params = new URLSearchParams(location.search);
  var cookieToken = (document.cookie.match(/(?:^|;\s*)caret_token=([^;]*)/) || [])[1];
  var token = params.get('token') || (cookieToken ? decodeURIComponent(cookieToken) : null);
  try {
    if (!token) token = localStorage.getItem('caret_token');
    if (token) localStorage.setItem('caret_token', token);
  } catch (e) { /* storage may be disabled */ }
  if (params.has('token') && window.history && history.replaceState) {
    params.delete('token');
    var clean = location.pathname + (params.toString() ? '?' + params.toString() : '') + location.hash;
    try { history.replaceState(null, '', clean); } catch (e) { /* ignore */ }
  }

  var state = { connected: false, token: token, url: location.origin, attempts: 0, clientId: null };
  window.__CARET_REMOTE__ = state;
  function emitState() {
    try { window.dispatchEvent(new CustomEvent('caret-remote', { detail: { connected: state.connected, attempts: state.attempts } })); } catch (e) { /* ignore */ }
  }

  // ─── callbacks (transformCallback) ───
  var callbacks = {};
  function randomId() {
    var id;
    do { id = (crypto && crypto.getRandomValues) ? crypto.getRandomValues(new Uint32Array(1))[0] : Math.floor(Math.random() * 4294967295); } while (!id || callbacks[id]);
    return id;
  }
  function transformCallback(cb, once) {
    var id = randomId();
    var fn = function (result) {
      if (once) unregisterCallback(id);
      return cb && cb(result);
    };
    callbacks[id] = fn;
    window['_' + id] = fn;
    return id;
  }
  function unregisterCallback(id) {
    delete callbacks[id];
    try { delete window['_' + id]; } catch (e) { /* ignore */ }
  }

  // ─── relay socket ───
  var ws = null;
  var pending = {};
  var queue = [];
  var nextId = 1;
  var reconnectTimer = null;
  var closedByUs = false;

  function wsUrl() {
    var proto = location.protocol === 'https:' ? 'wss://' : 'ws://';
    return proto + location.host + '/ws?role=client&token=' + encodeURIComponent(state.token || '');
  }

  function connect() {
    if (closedByUs) return;
    try { ws = new WebSocket(wsUrl()); } catch (e) { scheduleReconnect(); return; }
    ws.onopen = function () {
      state.connected = true; state.attempts = 0; emitState();
      var q = queue; queue = [];
      for (var i = 0; i < q.length; i++) ws.send(q[i]);
    };
    ws.onmessage = function (ev) {
      var msg;
      try { msg = JSON.parse(ev.data); } catch (e) { return; }
      if (!msg || typeof msg !== 'object') return;
      if (msg.t === 'reply') {
        var p = pending[msg.id];
        if (!p) return;
        delete pending[msg.id];
        if (msg.ok) p.resolve(msg.value); else p.reject(msg.error === undefined ? 'remote invoke failed' : msg.error);
      } else if (msg.t === 'cb') {
        var fn = callbacks[msg.callback];
        if (fn) { try { fn(msg.payload); } catch (e) { setTimeout(function () { throw e; }, 0); } }
      }
    };
    ws.onclose = function (ev) {
      var wasConnected = state.connected;
      state.connected = false; emitState();
      ws = null;
      // 4401/1008 = token rejected: go back to the pairing page instead of hammering the server.
      if (ev && (ev.code === 1008 || ev.code === 4401)) { failAll('remote token rejected'); location.reload(); return; }
      failAll(wasConnected ? 'remote connection lost' : 'remote connection failed');
      scheduleReconnect();
    };
    ws.onerror = function () { /* onclose follows */ };
  }
  function failAll(reason) {
    var ids = Object.keys(pending);
    for (var i = 0; i < ids.length; i++) { var p = pending[ids[i]]; delete pending[ids[i]]; p.reject(reason); }
  }
  function scheduleReconnect() {
    if (reconnectTimer || closedByUs) return;
    state.attempts += 1;
    var delay = Math.min(30000, 500 * Math.pow(2, Math.min(state.attempts, 6)));
    reconnectTimer = setTimeout(function () { reconnectTimer = null; connect(); }, delay);
  }
  function send(frame) {
    if (ws && ws.readyState === 1 && state.connected) ws.send(frame); else queue.push(frame);
  }

  // Commands that must stay in the browser: window chrome acts on the desktop window otherwise, zoom falls back to
  // CSS zoom in the app when the native call fails, process exit would kill the desktop.
  var LOCAL_NOOP = {
    'plugin:window|close': 1, 'plugin:window|destroy': 1, 'plugin:window|hide': 1, 'plugin:window|show': 1, 'plugin:window|minimize': 1,
    'plugin:window|unminimize': 1, 'plugin:window|maximize': 1, 'plugin:window|unmaximize': 1, 'plugin:window|toggle_maximize': 1,
    'plugin:window|set_focus': 1, 'plugin:window|start_dragging': 1, 'plugin:window|start_resize_dragging': 1, 'plugin:window|set_title': 1,
    'plugin:window|set_size': 1, 'plugin:window|set_position': 1, 'plugin:window|set_fullscreen': 1, 'plugin:window|request_user_attention': 1,
    'app_open_devtools': 1
  };
  var LOCAL_REJECT = {
    'plugin:webview|set_webview_zoom': 'zoom is applied in the browser',
    'plugin:process|exit': 'not available in a remote session',
    'plugin:process|restart': 'not available in a remote session'
  };

  function invoke(cmd, args, options) {
    if (args === undefined || args === null) args = {};
    if (LOCAL_NOOP[cmd]) return Promise.resolve(null);
    if (LOCAL_REJECT[cmd]) return Promise.reject(LOCAL_REJECT[cmd]);
    if (cmd === 'plugin:clipboard-manager|write_text' && navigator.clipboard && navigator.clipboard.writeText) {
      var text = args.text !== undefined ? args.text : (args.data && args.data.text);
      return navigator.clipboard.writeText(String(text == null ? '' : text)).then(function () { return null; }, function () { return relay(cmd, args, options); });
    }
    if (cmd === 'plugin:clipboard-manager|read_text' && navigator.clipboard && navigator.clipboard.readText) {
      return navigator.clipboard.readText().then(function (t) { return t; }, function () { return relay(cmd, args, options); });
    }
    return relay(cmd, args, options);
  }
  function relay(cmd, args, options) {
    return new Promise(function (resolve, reject) {
      var id = nextId++;
      var frame;
      try {
        // JSON.stringify calls Channel.toJSON() so channels travel as "__CHANNEL__:<id>" like in Tauri's IPC.
        frame = JSON.stringify({ t: 'invoke', id: id, cmd: cmd, args: args, options: options === undefined ? null : options });
      } catch (e) { reject(String(e)); return; }
      pending[id] = { resolve: resolve, reject: reject };
      send(frame);
    });
  }

  // ─── metadata / plugins ───
  var isMacClient = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
  var isWinClient = /Win/.test(navigator.platform || '');
  var platform = isMacClient ? 'macos' : isWinClient ? 'windows' : 'linux';
  window.__TAURI_INTERNALS__ = {
    invoke: invoke,
    transformCallback: transformCallback,
    unregisterCallback: unregisterCallback,
    convertFileSrc: function (path) { return path; },
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { label: 'main', windowLabel: 'main' },
      windows: [{ label: 'main' }],
      webviews: [{ label: 'main', windowLabel: 'main' }]
    },
    plugins: { path: { sep: '/', delimiter: ':' } }
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: function () { /* the executor owns the listener */ } };
  window.__TAURI_OS_PLUGIN_INTERNALS__ = {
    platform: platform, os_type: platform, family: isWinClient ? 'windows' : 'unix', version: '0', arch: 'unknown',
    eol: isWinClient ? '\r\n' : '\n', exe_extension: isWinClient ? 'exe' : '', locale: navigator.language || null
  };
  window.isTauri = true;

  function markBody() { if (document.body) document.body.setAttribute('data-remote', '1'); }
  document.documentElement.setAttribute('data-remote', '1');
  if (document.body) markBody(); else document.addEventListener('DOMContentLoaded', markBody);
  window.addEventListener('beforeunload', function () { closedByUs = true; if (ws) { try { ws.close(); } catch (e) { /* ignore */ } } });

  if (!state.token) { location.replace('/__remote/pair.html'); return; }
  connect();
})();
