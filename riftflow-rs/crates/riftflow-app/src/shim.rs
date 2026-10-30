//! The renderer compatibility shim, injected as an initialization script.
//!
//! Why this exists instead of editing the renderer: the Vue app reaches the main
//! process through exactly two places — `window.electron.ipcRenderer` (from
//! `@electron-toolkit/preload`, used by `call-service.ts:11` and
//! `renderer-shared/shards/ipc/index.ts:39`) and `window.riftflowWindowType`.
//! Emulating those two keeps all 77,493 lines of renderer code untouched.
//!
//! Contract kept verbatim:
//! * `invoke('riftflowCall', namespace, fnName, ...args)` resolves with the
//!   `{ success, data }` / `{ success: false, error }` envelope, and **rejects**
//!   for an unknown method (Electron rethrows there, it does not wrap).
//! * `invoke('riftflowRendererRegister', action)`.
//! * `on('riftflow-event', listener)` delivers `listener(event, namespace, eventName, ...args)`
//!   and returns a cancel function, like `@electron-toolkit/preload` does.

/// The renderer's `riftflow://` URLs, rewritten into the form WebView2 will request.
/// Kept out of the `format!` body above: this half has no placeholders and every
/// brace in it would need doubling.
pub const PROTOCOL_SCRIPT: &str = r#"(function () {
  // The renderer reaches the LCU through `riftflow://<domain>/<path>` urls. WebView2
  // only feeds a non-standard scheme to Tauri's protocol handler while the document
  // is still loading (measured: identical requests land at document start and fail
  // once it is complete), so fetch traffic is answered through the
  // `riftflow_protocol_fetch` command and resource urls become blob: urls of the same
  // bytes. The app's own urls and code stay untouched either way.
  var PREFIX = 'riftflow://';
  var LOCAL_FORM = location.protocol + '//riftflow.localhost/';
  var TRACE = __RIFTFLOW_PROTOCOL_TRACE__;
  var RIFTFLOW_IN_TEXT = /riftflow:\/\/[^"'()\s]+/g;

  function note(text) {
    if (!TRACE) return;
    try {
      window.__TAURI_INTERNALS__.invoke('shell_report', { note: String(text).slice(0, 220) });
    } catch (ignored) {}
  }

  // A url always starts with the scheme; anything else (a style attribute holding
  // `url(riftflow://…)`) is handled by the text path instead.
  function riftflowUrlOf(value) {
    if (typeof value !== 'string') return null;
    if (value.indexOf(PREFIX) === 0) return value;
    if (value.indexOf(LOCAL_FORM) === 0) return PREFIX + value.slice(LOCAL_FORM.length);
    return null;
  }

  function toBase64(bytes) {
    var binary = '';
    var chunk = 0x8000;
    for (var i = 0; i < bytes.length; i += chunk) {
      binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
    }
    return btoa(binary);
  }

  function fromBase64(text) {
    var binary = atob(text || '');
    var bytes = new Uint8Array(binary.length);
    for (var i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return bytes;
  }

  function headerPairsOf(source) {
    var pairs = [];
    if (!source) return pairs;
    if (typeof source.forEach === 'function') {
      source.forEach(function (value, name) { pairs.push([name, String(value)]); });
      return pairs;
    }
    Object.keys(source).forEach(function (name) { pairs.push([name, String(source[name])]); });
    return pairs;
  }

  function readBody(source, method) {
    if (method === 'GET' || method === 'HEAD') return Promise.resolve(undefined);
    var body = source && source.body;
    if (body === undefined || body === null) return Promise.resolve(undefined);
    if (typeof body === 'string') return Promise.resolve(toBase64(new TextEncoder().encode(body)));
    if (body instanceof Uint8Array) return Promise.resolve(toBase64(body));
    if (body instanceof ArrayBuffer) return Promise.resolve(toBase64(new Uint8Array(body)));
    if (typeof body.arrayBuffer === 'function') {
      return body.arrayBuffer().then(function (buffer) {
        return toBase64(new Uint8Array(buffer));
      });
    }
    if (typeof body.getReader === 'function') {
      return drainStream(body);
    }
    note('[protocol] body of type ' + Object.prototype.toString.call(body) + ' is not forwarded');
    return Promise.resolve(undefined);
  }

  function mustNotHaveBody(status) {
    return status === 204 || status === 205 || status === 304 || (status >= 100 && status < 200);
  }

  // axios' fetch adapter always wraps the body in a `Request`, whose `.body` is a
  // ReadableStream even for a plain JSON object, so draining streams is the one
  // path every POST through this proxy actually takes.
  function drainStream(stream) {
    var reader = stream.getReader();
    var chunks = [];
    var total = 0;

    function take() {
      return reader.read().then(function (result) {
        if (result.done) {
          var merged = new Uint8Array(total);
          var offset = 0;
          for (var i = 0; i < chunks.length; i++) {
            merged.set(chunks[i], offset);
            offset += chunks[i].length;
          }
          return toBase64(merged);
        }
        var chunk = result.value;
        if (chunk && chunk.length) {
          chunks.push(chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk));
          total += chunk.length;
        }
        return take();
      });
    }

    return take();
  }

  // One proxied request, answered exactly like the Electron protocol handler would.
  function proxyFetch(riftflow, source, init) {
    var method = String((init && init.method) || (source && source.method) || 'GET').toUpperCase();
    var headers = headerPairsOf((init && init.headers) || (source && source.headers));
    var bodySource = (init && init.body !== undefined && init.body !== null) ? init : source;

    return readBody(bodySource, method).then(function (body) {
      return window.__riftflowInvoke('riftflow_protocol_fetch', {
        url: riftflow,
        method: method,
        headers: headers,
        body: body,
      });
    }).then(function (result) {
      var map = {};
      (result.headers || []).forEach(function (pair) { map[pair[0]] = pair[1]; });
      var bytes = mustNotHaveBody(result.status) ? null : fromBase64(result.body);
      var signal = (init && init.signal) || (source && source.signal);
      if (signal && signal.aborted) {
        // the proxy cannot be cut off mid-flight, but the caller must see a cancel
        throw new DOMException('The request was aborted', 'AbortError');
      }
      return new Response(bytes, {
        status: result.status,
        statusText: result.statusText || '',
        headers: map,
      });
    }).catch(function (error) {
      // Rust answers a cancelled proxy with a rejected command; axios' fetch
      // adapter surfaces an abort as an AbortError, so keep that shape
      if (String((error && error.message) || error).indexOf('aborted') >= 0) {
        throw new DOMException('The request was aborted', 'AbortError');
      }
      throw error;
    });
  }

  var native_fetch = window.fetch;
  if (native_fetch && window.__riftflowInvoke) {
    window.fetch = function (input, init) {
      var riftflow = riftflowUrlOf(typeof input === 'string' ? input : input && input.url);
      if (!riftflow) return native_fetch.call(this, input, init);
      note('[protocol-fetch] ' + riftflow.slice(0, 140));
      return proxyFetch(riftflow, input, init);
    };
  }

  // Subresources cannot be answered from fetch, so their bytes become a blob url,
  // cached per url so one icon is not fetched once per <img>.
  var blobPromises = {};

  function blobFor(riftflow) {
    if (blobPromises[riftflow]) return blobPromises[riftflow];
    var promise = proxyFetch(riftflow, null, { method: 'GET' })
      .then(function (response) { return response.blob(); })
      .then(function (blob) { return URL.createObjectURL(blob); })
      .catch(function () {
        note('[protocol-blob] failed for ' + riftflow.slice(0, 140));
        return null;
      });
    blobPromises[riftflow] = promise;
    return promise;
  }

  // `xlink:href` is a legal attribute name for getAttribute/setAttribute but not a
  // valid CSS selector, so the scan list is narrower than the rewrite list.
  var WATCHED = ['src', 'href', 'srcset', 'poster', 'style', 'xlink:href'];
  var SELECTED = ['src', 'href', 'srcset', 'poster', 'style'];

  function resolveAttribute(element, name, value) {
    var riftflow = riftflowUrlOf(value);
    if (riftflow) {
      blobFor(riftflow).then(function (url) {
        if (url && element.getAttribute(name) === value) element.setAttribute(name, url);
      });
      return true;
    }
    if (value.indexOf(PREFIX) >= 0) return resolveText(element, name, value);
    return false;
  }

  // A style attribute carries the url inside text(), so every occurrence is
  // swapped for its blob url once the bytes are in.
  function resolveText(element, name, value) {
    var found = value.match(RIFTFLOW_IN_TEXT) || [];
    if (!found.length) return false;
    Promise.all(found.map(blobFor)).then(function (urls) {
      var next = value;
      found.forEach(function (riftflow, index) {
        if (urls[index]) next = next.split(riftflow).join(urls[index]);
      });
      if (next !== value && element.getAttribute(name) === value) element.setAttribute(name, next);
    });
    return true;
  }

  function rewriteElement(element) {
    if (!element || element.nodeType !== 1 || !element.getAttribute) return;
    for (var i = 0; i < WATCHED.length; i++) {
      var name = WATCHED[i];
      var value = element.getAttribute(name);
      if (value && value.indexOf(PREFIX) >= 0) resolveAttribute(element, name, value);
    }
  }

  function scan(node) {
    if (!node || node.nodeType !== 1) return;
    rewriteElement(node);
    var found = node.querySelectorAll
      ? node.querySelectorAll('[' + SELECTED.join('],[') + ']')
      : [];
    for (var i = 0; i < found.length; i++) rewriteElement(found[i]);
  }

  // An `<img>` that is never attached to the document still has to load.
  function patchProperty(prototype, name) {
    if (!prototype) return;
    var descriptor = Object.getOwnPropertyDescriptor(prototype, name);
    if (!descriptor || !descriptor.set) return;
    Object.defineProperty(prototype, name, {
      get: function () { return descriptor.get.call(this); },
      set: function (value) {
        var riftflow = riftflowUrlOf(value);
        if (riftflow) {
          var element = this;
          blobFor(riftflow).then(function (url) {
            descriptor.set.call(element, url || value);
          });
          return;
        }
        descriptor.set.call(this, value);
      },
      configurable: true,
      enumerable: descriptor.enumerable,
    });
  }

  patchProperty(window.HTMLImageElement && HTMLImageElement.prototype, 'src');
  patchProperty(window.HTMLScriptElement && HTMLScriptElement.prototype, 'src');
  patchProperty(window.HTMLLinkElement && HTMLLinkElement.prototype, 'href');
  patchProperty(window.HTMLMediaElement && HTMLMediaElement.prototype, 'src');

  // XHR keeps the http form of the url: the riftflow protocol handler answers it while
  // WebView2 intercepts. The shards that use XHR for `riftflow://` (sgp, riot-client)
  // are not ported yet, so this path has no live traffic to lose today.
  if (window.XMLHttpRequest) {
    var native_open = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (method, url) {
      var rest = Array.prototype.slice.call(arguments, 2);
      var riftflow = riftflowUrlOf(url);
      var next = riftflow ? LOCAL_FORM + riftflow.slice(PREFIX.length) : url;
      return native_open.apply(this, [method, next].concat(rest));
    };
  }

  function start() {
    if (!document.documentElement) {
      setTimeout(start, 10);
      return;
    }
    var observer = new MutationObserver(function (records) {
      for (var i = 0; i < records.length; i++) {
        var record = records[i];
        try {
          if (record.type === 'attributes') scan(record.target);
          else if (record.type === 'childList') {
            for (var j = 0; j < record.addedNodes.length; j++) scan(record.addedNodes[j]);
          }
        } catch (error) {
          note('[dom-scan] failed: ' + ((error && error.message) || error));
        }
      }
    });
    observer.observe(document.documentElement, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: SELECTED,
    });
    scan(document.documentElement);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', start);
  } else {
    start();
  }

  if (TRACE && window.__riftflowInvoke) {
    // One LCU request through the proxy, reported to stderr, so the chain
    // (patch -> command -> LCU -> Response) is provable without app code.
    window.__riftflowInvoke('riftflow_protocol_fetch', {
      url: PREFIX + 'league-client/lol-summoner/v1/current-summoner',
      method: 'GET',
      headers: [],
      body: undefined,
    }).then(function (result) {
      note('[proxy-selftest] status=' + result.status + ' bytes=' + String(result.body || '').length);
    }, function (error) {
      note('[proxy-selftest] failed: ' + ((error && error.message) || error));
    });
  }
})();"#;

/// The script body, with the window type interpolated for `window.riftflowWindowType`.
/// The `riftflow://` rewrite is appended, so one injected string covers both seams.
pub fn initialization_script(window_type: &str) -> String {
    format!(

        r#"(function () {{
  if (window.__riftflowShimInstalled) return;
  window.__riftflowShimInstalled = true;

  var WINDOW_TYPE = {window_type:?};
  var listeners = {{}};

  function internals() {{
    return window.__TAURI_INTERNALS__;
  }}

  // Tauri's IPC object is installed by its own initialization script; wait for it
  // rather than assuming ordering across injected scripts.
  function ready() {{
    if (internals()) return Promise.resolve();
    return new Promise(function (resolve, reject) {{
      var attempts = 0;
      (function poll() {{
        if (internals()) return resolve();
        if (++attempts > 400) return reject(new Error('Tauri IPC bridge unavailable'));
        setTimeout(poll, 5);
      }})();
    }});
  }}

  function callRust(command, args) {{
    return ready().then(function () {{
      return internals().invoke(command, args);
    }});
  }}

  // Rust pushes these props as JSON, so a `Set`-typed prop arrives marked; the
  // renderer's store declares it as a real Set and calls `.has()` on it.
  function reviveSet(value) {{
    var marked = value && value.__riftflowSet__;
    return Array.isArray(marked) ? new Set(marked) : value;
  }}

  function reviveSetProps(response) {{
    // `riftflow_call` answers with the {{ success, data }} envelope; the renderer's
    // ipc layer unwraps `data` after this has already run
    var initial = response && Object.prototype.hasOwnProperty.call(response, 'data')
      ? response.data
      : response;
    if (!initial || typeof initial !== 'object') return response;

    Object.keys(initial).forEach(function (path) {{
      var entry = initial[path];
      if (entry && typeof entry === 'object' && 'value' in entry) {{
        entry.value = reviveSet(entry.value);
      }}
    }});
    return response;
  }}

  // the protocol half below needs the same bridge
  window.__riftflowInvoke = callRust;

  window.electron = {{
    ipcRenderer: {{
      invoke: function (channel) {{
        var rest = Array.prototype.slice.call(arguments, 1);

        if (channel === 'riftflowCall') {{
          var call = callRust('riftflow_call', {{
            namespace: rest[0],
            fnName: rest[1],
            args: rest.slice(2),
          }});
          if (rest[0] === 'mobx-utils-main' && rest[1] === 'subscribeAndGetInitialState') {{
            return call.then(reviveSetProps);
          }}
          return call;
        }}

        if (channel === 'riftflowRendererRegister') {{
          return callRust('riftflow_renderer_register', {{ action: rest[0] || 'register' }});
        }}

        // everything else stays available as a plain command, e.g. window ops
        return callRust(channel, {{ args: rest }});
      }},

      on: function (channel, listener) {{
        (listeners[channel] || (listeners[channel] = [])).push(listener);
        window.__riftflowListenerKeys = Object.keys(listeners).join(',');
        window.__riftflowListenerCount = (window.__riftflowListenerCount || 0) + 1;
        return function () {{
          window.electron.ipcRenderer.removeListener(channel, listener);
        }};
      }},

      removeListener: function (channel, listener) {{
        var arr = listeners[channel];
        if (!arr) return;
        var at = arr.indexOf(listener);
        if (at >= 0) arr.splice(at, 1);
      }},

      send: function (channel) {{
        var rest = Array.prototype.slice.call(arguments, 1);
        return callRust(channel + '_send', {{ args: rest }}).catch(function () {{}});
      }},

      once: function (channel, listener) {{
        var wrapped = function () {{
          window.electron.ipcRenderer.removeListener(channel, wrapped);
          listener.apply(null, arguments);
        }};
        return window.electron.ipcRenderer.on(channel, wrapped);
      }},
    }},
    process: {{ versions: {{ electron: 'rust-riftflow' }} }},
  }};

  // Rust pushes events with `eval`, which keeps this off Tauri's private event
  // protocol: __riftflowDispatch('riftflow-event', [namespace, eventName, ...args])
  window.__riftflowDispatch = function (channel, args) {{
    var framed = args || [];
    // `[namespace, eventName, path, value, {{ action, raw }}]` for a state push
    if (channel === 'riftflow-event' && typeof framed[1] === 'string' &&
        framed[1].indexOf('update-state-prop/') === 0) {{
      framed = framed.slice();
      framed[3] = reviveSet(framed[3]);
    }}

    var arr = listeners[channel];
    if (!arr || !arr.length) return;
    // the first parameter is the Electron event object; the app only reads
    // `_event` positionally, so a minimal stand-in is enough
    var fakeEvent = {{
      sender: {{ id: WINDOW_TYPE, send: function () {{}} }},
      senderId: 0,
      ports: [],
    }};
    var snapshot = arr.slice();
    for (var i = 0; i < snapshot.length; i++) {{
      try {{
        snapshot[i].apply(null, [fakeEvent].concat(framed));
      }} catch (error) {{
        console.error('riftflow-event listener failed', error);
      }}
    }}
  }};

  Object.defineProperty(window, 'riftflowWindowType', {{
    value: WINDOW_TYPE,
    writable: false,
    configurable: false,
  }});

  // The shell's WH_KEYBOARD_LL hook is never called for a keystroke whose target window
  // belongs to this process (measured: the page sees `keydown KeyJ:74` while the hook
  // logs nothing), so the renderer relays its own key stream into the same shortcut
  // pipeline — see `hotkeys::process_external_key_event`, which drops anything the hook
  // did report. Only the virtual-key code and the direction cross the boundary; the
  // chord state stays in Rust.
  var heldKeyCodes = {{}};

  function forwardKey(vk, isDown) {{
    try {{
      callRust('riftflow_key_event', {{ keyCode: vk, isDown: isDown }});
    }} catch (ignored) {{}}
  }}

  function sideSpecificCode(event) {{
    var code = event.keyCode;
    if (code !== 16 && code !== 17 && code !== 18) return code;
    // KeyboardEvent.location: 1 = left, 2 = right. 160/161 Shift, 162/163 Ctrl,
    // 164/165 Alt - the same side-specific codes the hook reports.
    var right = event.location === 2;
    if (code === 16) return right ? 161 : 160;
    if (code === 17) return right ? 163 : 162;
    return right ? 165 : 164;
  }}

  function relayKey(event) {{
    var isDown = event.type === 'keydown';
    if (isDown && event.repeat) return;
    var vk = sideSpecificCode(event);
    if (isDown) heldKeyCodes[vk] = true; else delete heldKeyCodes[vk];
    forwardKey(vk, isDown);
  }}

  window.addEventListener('keydown', relayKey, true);
  window.addEventListener('keyup', relayKey, true);
  // an alt-tab mid-chord loses the key-up, which would leave the modifier held
  window.addEventListener('blur', function () {{
    for (var vk in heldKeyCodes) forwardKey(Number(vk), false);
    heldKeyCodes = {{}};
  }});

  // Stand-in for the `renderer-debug` shard's error surfacing: without it a failed
  // renderer boot is invisible from the main process.
  function report_js(kind, detail) {{
    try {{
      window.__TAURI_INTERNALS__.invoke('shell_report', {{
        note: kind + ' ' + String(detail).slice(0, 400)
      }});
    }} catch (ignored) {{}}
  }}

  window.addEventListener('error', function (event) {{
    report_js('[js-error]', (event.message || 'error') + ' @ ' + (event.filename || '?') + ':' + (event.lineno || 0));
  }});

  window.addEventListener('unhandledrejection', function (event) {{
    var reason = event.reason;
    report_js('[js-reject]', (reason && (reason.stack || reason.message)) || reason);
  }});

  var original_console_error = console.error;
  console.error = function () {{
    report_js('[console.error]', Array.prototype.map.call(arguments, function (a) {{
      if (a instanceof Error) return a.name + ': ' + a.message + '\n' + (a.stack || '');
      if (typeof a === 'string') return a;
      try {{ return JSON.stringify(a); }} catch (ignored) {{ return String(a); }}
    }}).join(' '));
    return original_console_error.apply(console, arguments);
  }};
}})();"#
    ) + &PROTOCOL_SCRIPT.replace(
        "__RIFTFLOW_PROTOCOL_TRACE__",
        if std::env::var("RIFTFLOW_IPC_TRACE").is_ok() { "true" } else { "false" },
    )
}
