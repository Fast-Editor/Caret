// Injected into every page of a Caret browser window (initialization_script).
// Exposes window.__caret: snapshot/click/type/eval + console capture. Results are delivered to Rust
// through blocked navigations to the `caret-result:` scheme (no IPC access is granted to remote pages).
(function () {
  if (window.__caret && window.__caret.__installed) return;
  const logs = [];
  const pushLog = (level, args) => {
    try { logs.push(`[${level}] ${Array.from(args).map(a => { try { return typeof a === 'string' ? a : JSON.stringify(a); } catch { return String(a); } }).join(' ')}`.slice(0, 2000)); } catch {}
    if (logs.length > 200) logs.shift();
  };
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
    const orig = console[level];
    console[level] = function () { pushLog(level, arguments); try { orig && orig.apply(console, arguments); } catch {} };
  }
  window.addEventListener('error', (e) => pushLog('error', [e.message + ' @' + (e.filename || '') + ':' + (e.lineno || '')]));
  window.addEventListener('unhandledrejection', (e) => pushLog('error', ['unhandledrejection: ' + (e.reason && (e.reason.stack || e.reason.message) || e.reason)]));

  const refs = new Map(); let refCounter = 0;
  const INTERACTIVE = 'a[href], button, input, textarea, select, summary, [role="button"], [role="link"], [role="tab"], [role="menuitem"], [role="checkbox"], [role="textbox"], [onclick], [contenteditable=""], [contenteditable="true"]';
  const visible = (el) => { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none'; };
  const textOf = (el) => (el.innerText || el.textContent || el.getAttribute('aria-label') || el.getAttribute('title') || el.value || '').trim().replace(/\s+/g, ' ').slice(0, 160);

  function send(id, ok, value) {
    const payload = JSON.stringify({ ok, value });
    const b64 = btoa(unescape(encodeURIComponent(payload)));
    const CH = 24000; const total = Math.ceil(b64.length / CH) || 1;
    let i = 0;
    const step = () => {
      if (i >= total) return;
      const part = b64.slice(i * CH, (i + 1) * CH);
      // Each navigation is intercepted and cancelled by the Rust side.
      try { location.assign('caret-result://r/?id=' + encodeURIComponent(id) + '&seq=' + i + '&total=' + total + '&data=' + encodeURIComponent(part)); } catch (e) {}
      i++; if (i < total) setTimeout(step, 15);
    };
    step();
  }

  const api = {
    __installed: true,
    logs: () => logs.slice(),
    snapshot() {
      refs.clear(); refCounter = 0;
      const elements = [];
      for (const el of document.querySelectorAll(INTERACTIVE)) {
        if (!visible(el)) continue;
        const ref = 'e' + (++refCounter);
        refs.set(ref, el); el.setAttribute('data-caret-ref', ref);
        const tag = el.tagName.toLowerCase();
        const item = { ref, tag, text: textOf(el) };
        const role = el.getAttribute('role'); if (role) item.role = role;
        if (tag === 'a' && el.href) item.href = el.href;
        if (el.placeholder) item.placeholder = el.placeholder;
        if ((tag === 'input' || tag === 'textarea' || tag === 'select') && el.value != null && String(el.value).length) item.value = String(el.value).slice(0, 200);
        if (el.name) item.name = el.name; if (tag === 'input' && el.type) item.role = item.role || el.type;
        elements.push(item);
        if (elements.length >= 400) break;
      }
      const text = (document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n').slice(0, 20000);
      return { url: location.href, title: document.title, text, elements, consoleLogs: logs.slice(-50) };
    },
    find(ref) { return refs.get(ref) || document.querySelector('[data-caret-ref="' + ref + '"]') || document.querySelector(ref); },
    click(ref) {
      const el = api.find(ref); if (!el) throw new Error('element not found: ' + ref);
      el.scrollIntoView({ block: 'center' }); el.focus && el.focus();
      const opts = { bubbles: true, cancelable: true, view: window };
      el.dispatchEvent(new MouseEvent('mousedown', opts)); el.dispatchEvent(new MouseEvent('mouseup', opts)); el.click ? el.click() : el.dispatchEvent(new MouseEvent('click', opts));
      return true;
    },
    type(ref, text, submit) {
      const el = api.find(ref); if (!el) throw new Error('element not found: ' + ref);
      el.focus && el.focus();
      if (el.isContentEditable) { el.textContent = text; el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text })); }
      else { const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; const setter = Object.getOwnPropertyDescriptor(proto, 'value'); setter && setter.set ? setter.set.call(el, text) : (el.value = text); el.dispatchEvent(new Event('input', { bubbles: true })); el.dispatchEvent(new Event('change', { bubbles: true })); }
      if (submit) { const kd = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true }; el.dispatchEvent(new KeyboardEvent('keydown', kd)); el.dispatchEvent(new KeyboardEvent('keypress', kd)); el.dispatchEvent(new KeyboardEvent('keyup', kd)); const f = el.form; if (f && !kd.defaultPrevented) { if (f.requestSubmit) f.requestSubmit(); else f.submit(); } }
      return true;
    },
    run(id, fn) {
      Promise.resolve().then(fn).then((v) => send(id, true, v === undefined ? null : v), (e) => send(id, false, (e && (e.stack || e.message)) || String(e)));
    },
  };
  window.__caret = api;
})();
