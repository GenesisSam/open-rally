(function () {
  'use strict';

  const PAGE = 100;
  const $ = (s) => document.querySelector(s);
  const list = $('#list');
  const sentinel = $('#top-sentinel');
  const dot = $('#dot');
  const textEl = $('#text');
  const sendBtn = $('#send');
  const chipsEl = $('#chips');
  const fileInput = $('#file-input');
  const progress = $('#progress');
  const bar = $('#bar');
  const loginEl = $('#login');
  const loginMsg = $('#login-msg');
  const toastEl = $('#toast');
  const dropHint = $('#drop-hint');

  const st = {
    posts: new Map(),   // id -> element
    oldest: null,        // 가장 오래된 글 id (위로 더 불러오기용)
    exhausted: false,
    loadingOlder: false,
    pendingFiles: [],
    ws: null,
    backoff: 1000,
    everConnected: false,
    reconnectTimer: null,
    sending: false,
  };

  $('#host').textContent = location.host;

  // ---------- 유틸 ----------
  function toast(msg, ms) {
    toastEl.textContent = msg;
    toastEl.classList.remove('hidden');
    clearTimeout(toast.t);
    toast.t = setTimeout(() => toastEl.classList.add('hidden'), ms || 2000);
  }

  function pad(n) { return n < 10 ? '0' + n : '' + n; }
  function fmtTime(ms) {
    const d = new Date(ms);
    const now = new Date();
    const hm = pad(d.getHours()) + ':' + pad(d.getMinutes());
    const sameDay = d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth() && d.getDate() === now.getDate();
    return sameDay ? hm : pad(d.getMonth() + 1) + '/' + pad(d.getDate()) + ' ' + hm;
  }
  function fmtSize(n) {
    if (n < 1024) return n + ' B';
    if (n < 1024 * 1024) return (n / 1024).toFixed(1) + ' KB';
    if (n < 1024 * 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + ' MB';
    return (n / 1024 / 1024 / 1024).toFixed(2) + ' GB';
  }
  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function nearBottom() {
    return list.scrollHeight - list.scrollTop - list.clientHeight < 80;
  }
  function scrollBottom() { list.scrollTop = list.scrollHeight; }

  // ---------- 복사 ----------
  function copyText(pre, btn) {
    const sel = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(pre);
    sel.removeAllRanges();
    sel.addRange(range);
    let ok = false;
    try { ok = document.execCommand('copy'); } catch (e) { ok = false; }
    if (ok) {
      sel.removeAllRanges();
      btn.textContent = '복사됨';
      btn.classList.add('done');
      setTimeout(() => { btn.textContent = '복사'; btn.classList.remove('done'); }, 1500);
    } else {
      toast('자동 복사가 막혀 있습니다. 선택된 텍스트를 ⌘C / Ctrl+C 로 복사하세요', 4000);
    }
  }

  // ---------- 렌더 ----------
  function fileCard(att) {
    const card = el('div', 'file-card');
    card.appendChild(el('div', 'ico', '⬇'));
    const nm = el('div', 'nm');
    nm.appendChild(document.createTextNode(att.filename));
    nm.appendChild(el('small', null, fmtSize(att.size) + ' · ' + att.mime));
    card.appendChild(nm);
    const a = el('a', null, '다운로드');
    a.href = att.url;
    a.setAttribute('download', att.filename);
    card.appendChild(a);
    return card;
  }

  function purgedCard(att) {
    const card = el('div', 'file-card purged');
    card.appendChild(el('div', 'ico', '🗑'));
    const nm = el('div', 'nm');
    nm.appendChild(document.createTextNode(att.filename));
    nm.appendChild(el('small', null, fmtSize(att.size) + ' · 자동 정리로 삭제됨'));
    card.appendChild(nm);
    return card;
  }

  function attachment(att) {
    const wrap = el('div', 'att');
    if (att.purged) {
      wrap.appendChild(purgedCard(att));
    } else if (att.kind === 'image') {
      const img = el('img');
      img.src = att.url;
      img.alt = att.filename;
      img.loading = 'lazy';
      img.addEventListener('click', () => window.open(att.url, '_blank'));
      img.addEventListener('error', () => { wrap.replaceChildren(fileCard(att)); });
      wrap.appendChild(img);
    } else if (att.kind === 'video') {
      const v = el('video');
      v.controls = true;
      v.preload = 'metadata';
      v.src = att.url;
      v.addEventListener('error', () => { wrap.replaceChildren(fileCard(att)); });
      wrap.appendChild(v);
    } else {
      wrap.appendChild(fileCard(att));
    }
    return wrap;
  }

  function renderPost(post) {
    const card = el('div', 'post');
    card.dataset.id = post.id;
    const meta = el('div', 'meta');
    meta.appendChild(el('span', null, fmtTime(post.created_at)));
    const del = el('button', null, '삭제');
    del.addEventListener('click', () => deletePost(post.id));
    meta.appendChild(del);
    card.appendChild(meta);

    if (post.body) {
      const wrap = el('div', 'text-wrap');
      const pre = el('pre', null, post.body);
      const btn = el('button', 'copy-btn', '복사');
      btn.addEventListener('click', () => copyText(pre, btn));
      wrap.appendChild(pre);
      wrap.appendChild(btn);
      card.appendChild(wrap);
    }
    (post.attachments || []).forEach((a) => card.appendChild(attachment(a)));
    return card;
  }

  function appendPost(post) {
    if (st.posts.has(post.id)) return;
    const stick = nearBottom();
    const node = renderPost(post);
    list.appendChild(node);
    st.posts.set(post.id, node);
    if (!st.oldest || post.id < st.oldest) st.oldest = post.id;
    if (stick) scrollBottom();
  }

  function removePost(id) {
    const node = st.posts.get(id);
    if (node) { node.remove(); st.posts.delete(id); }
  }

  function updatePost(post) {
    const old = st.posts.get(post.id);
    if (!old) return;
    const node = renderPost(post);
    old.replaceWith(node);
    st.posts.set(post.id, node);
  }

  function showRetention(days) {
    const note = $('#retention');
    note.textContent = days > 0 ? '첨부 ' + days + '일 후 자동 삭제' : '';
  }

  async function loadSettings() {
    try {
      const r = await api('/api/settings');
      const s = await r.json();
      showRetention(s.retention_days || 0);
    } catch (e) { /* 401 은 api 가 처리 */ }
  }

  function replaceAll(posts) {
    list.replaceChildren(sentinel);
    st.posts.clear();
    st.oldest = null;
    st.exhausted = posts.length < PAGE;
    posts.slice().reverse().forEach((p) => {
      const node = renderPost(p);
      list.appendChild(node);
      st.posts.set(p.id, node);
      if (!st.oldest || p.id < st.oldest) st.oldest = p.id;
    });
    scrollBottom();
  }

  // ---------- API ----------
  async function api(path, opts) {
    const r = await fetch(path, Object.assign({ credentials: 'same-origin' }, opts || {}));
    if (r.status === 401) { showLogin(''); throw new Error('unauthorized'); }
    return r;
  }

  async function loadInitial() {
    const r = await api('/api/posts?limit=' + PAGE);
    const data = await r.json();
    replaceAll(data.posts);
  }

  async function loadOlder() {
    if (st.loadingOlder || st.exhausted || !st.oldest) return;
    st.loadingOlder = true;
    const hint = el('div', 'loading-older', '이전 글 불러오는 중…');
    sentinel.after(hint);
    try {
      const r = await api('/api/posts?limit=' + PAGE + '&before=' + encodeURIComponent(st.oldest));
      const data = await r.json();
      const before = list.scrollHeight;
      hint.remove();
      if (data.posts.length < PAGE) st.exhausted = true;
      data.posts.forEach((p) => {       // 최신순으로 오므로 하나씩 sentinel 바로 뒤에 끼운다
        if (st.posts.has(p.id)) return;
        const node = renderPost(p);
        sentinel.after(node);
        st.posts.set(p.id, node);
        if (!st.oldest || p.id < st.oldest) st.oldest = p.id;
      });
      list.scrollTop += list.scrollHeight - before;
    } catch (e) {
      hint.remove();
    } finally {
      st.loadingOlder = false;
    }
  }

  list.addEventListener('scroll', () => {
    if (list.scrollTop < 40) loadOlder();
  });

  async function deletePost(id) {
    if (!confirm('이 글을 삭제할까요? 첨부 파일도 함께 지워집니다.')) return;
    try {
      const r = await api('/api/posts/' + encodeURIComponent(id), { method: 'DELETE' });
      if (r.status === 204 || r.status === 404) removePost(id);
      else toast('삭제 실패 (' + r.status + ')');
    } catch (e) { /* 401 은 api 가 처리 */ }
  }

  // ---------- 로그인 ----------
  function showLogin(msg) {
    loginMsg.textContent = msg || '';
    loginEl.classList.remove('hidden');
    setTimeout(() => $('#pw').focus(), 0);
  }

  $('#login-form').addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const pw = $('#pw').value;
    if (!pw) return;
    const r = await fetch('/api/login', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ password: pw }),
    });
    if (r.status === 204) {
      $('#pw').value = '';
      loginEl.classList.add('hidden');
      await start();
    } else if (r.status === 429) {
      loginMsg.textContent = '시도가 많습니다. 10초 뒤 다시 해보세요';
    } else {
      loginMsg.textContent = '비밀번호가 틀렸습니다';
    }
  });

  // ---------- WebSocket ----------
  function setDot(on) {
    dot.classList.toggle('on', on);
    dot.classList.toggle('off', !on);
  }

  function connectWs() {
    const proto = location.protocol === 'https:' ? 'wss://' : 'ws://';
    const ws = new WebSocket(proto + location.host + '/ws');
    st.ws = ws;
    ws.onopen = async () => {
      setDot(true);
      st.backoff = 1000;
      if (st.everConnected) {
        // 끊긴 동안 생기거나 지워진 글을 반영하기 위해 전체를 다시 받는다.
        try { await loadInitial(); await loadSettings(); } catch (e) { /* 401 처리됨 */ }
      }
      st.everConnected = true;
    };
    ws.onmessage = (ev) => {
      let msg;
      try { msg = JSON.parse(ev.data); } catch (e) { return; }
      if (msg.type === 'created') appendPost(msg.post);
      else if (msg.type === 'deleted') removePost(msg.id);
      else if (msg.type === 'updated') updatePost(msg.post);
    };
    ws.onclose = () => {
      setDot(false);
      st.ws = null;
      scheduleReconnect();
    };
    ws.onerror = () => { /* onclose 가 뒤따른다 */ };
  }

  function scheduleReconnect() {
    clearTimeout(st.reconnectTimer);
    st.reconnectTimer = setTimeout(async () => {
      // 서버가 다시 켜졌으면 세션이 사라졌을 수 있다. 먼저 확인한다.
      try {
        const r = await fetch('/api/session', { credentials: 'same-origin' });
        if (r.status === 401) { showLogin('서버가 다시 시작됐습니다. 비밀번호를 다시 입력하세요'); return; }
        if (r.status !== 204) throw new Error('not ready');
        connectWs();
      } catch (e) {
        st.backoff = Math.min(st.backoff * 2, 30000);
        scheduleReconnect();
      }
    }, st.backoff);
  }

  // ---------- 입력 ----------
  function updateSendState() {
    sendBtn.disabled = st.sending || (!textEl.value.trim() && st.pendingFiles.length === 0);
  }

  function renderChips() {
    chipsEl.replaceChildren();
    st.pendingFiles.forEach((f, i) => {
      const chip = el('span', 'chip');
      chip.appendChild(el('span', null, f.name + ' (' + fmtSize(f.size) + ')'));
      const x = el('button', null, '×');
      x.title = '제거';
      x.addEventListener('click', () => { st.pendingFiles.splice(i, 1); renderChips(); updateSendState(); });
      chip.appendChild(x);
      chipsEl.appendChild(chip);
    });
  }

  function addFiles(files) {
    let n = 0;
    for (const f of files) {
      if (!f || typeof f.name !== 'string') continue;
      const name = f.name || ('clipboard-' + Date.now() + (f.type && f.type.startsWith('image/') ? '.png' : ''));
      st.pendingFiles.push(name === f.name ? f : new File([f], name, { type: f.type }));
      n++;
    }
    if (n) { renderChips(); updateSendState(); }
  }

  textEl.addEventListener('input', updateSendState);
  textEl.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter' && (ev.metaKey || ev.ctrlKey)) { ev.preventDefault(); send(); }
  });
  fileInput.addEventListener('change', () => { addFiles(fileInput.files); fileInput.value = ''; });
  sendBtn.addEventListener('click', send);

  document.addEventListener('paste', (ev) => {
    const files = ev.clipboardData && ev.clipboardData.files;
    if (files && files.length) { ev.preventDefault(); addFiles(files); }
  });

  let dragDepth = 0;
  document.addEventListener('dragenter', (ev) => {
    if (!ev.dataTransfer || !Array.from(ev.dataTransfer.types).includes('Files')) return;
    dragDepth++;
    dropHint.classList.remove('hidden');
  });
  document.addEventListener('dragleave', () => {
    if (--dragDepth <= 0) { dragDepth = 0; dropHint.classList.add('hidden'); }
  });
  document.addEventListener('dragover', (ev) => { ev.preventDefault(); });
  document.addEventListener('drop', (ev) => {
    ev.preventDefault();
    dragDepth = 0;
    dropHint.classList.add('hidden');
    if (ev.dataTransfer && ev.dataTransfer.files.length) addFiles(ev.dataTransfer.files);
  });

  function send() {
    if (sendBtn.disabled) return;
    const text = textEl.value;
    const files = st.pendingFiles.slice();
    if (!text.trim() && files.length === 0) return;

    const fd = new FormData();
    if (text.trim()) fd.append('body', text);
    files.forEach((f) => fd.append('files', f, f.name));

    st.sending = true;
    updateSendState();
    const hasFiles = files.length > 0;
    if (hasFiles) { bar.style.width = '0%'; progress.classList.remove('hidden'); }

    const xhr = new XMLHttpRequest();
    xhr.open('POST', '/api/posts');
    xhr.withCredentials = true;
    xhr.upload.onprogress = (ev) => {
      if (ev.lengthComputable) bar.style.width = Math.round(ev.loaded / ev.total * 100) + '%';
    };
    xhr.onload = () => {
      st.sending = false;
      progress.classList.add('hidden');
      if (xhr.status === 201) {
        textEl.value = '';
        st.pendingFiles = [];
        renderChips();
        try { appendPost(JSON.parse(xhr.responseText)); } catch (e) { /* ws 로도 온다 */ }
        scrollBottom();
        textEl.focus();
      } else if (xhr.status === 401) {
        showLogin('');
      } else {
        toast('올리기 실패 (' + xhr.status + ')');
      }
      updateSendState();
    };
    xhr.onerror = () => {
      st.sending = false;
      progress.classList.add('hidden');
      toast('올리기 실패: 서버에 연결할 수 없습니다');
      updateSendState();
    };
    xhr.send(fd);
  }

  // ---------- 시작 ----------
  async function start() {
    try {
      await loadInitial();
      await loadSettings();
    } catch (e) {
      return; // 401 → 로그인 화면
    }
    clearTimeout(st.reconnectTimer);
    if (st.ws) { try { st.ws.onclose = null; st.ws.close(); } catch (e) { /* ignore */ } }
    st.everConnected = false;
    connectWs();
    textEl.focus();
  }

  (async () => {
    const r = await fetch('/api/session', { credentials: 'same-origin' });
    if (r.status === 204) await start();
    else showLogin('');
  })();
})();
