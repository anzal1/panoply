// Ported unchanged from the approved single-file page.
(function(){
  var root = document.documentElement;
  var still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  var REL = 'https://github.com/anzal1/panoply/releases/download/v0.3.0-beta/';

  // Name the download after the visitor's system.
  var ua = navigator.userAgent;
  var os = /Windows/.test(ua) ? 'win' : /Linux|X11/.test(ua) && !/Android/.test(ua) ? 'linux' : 'mac';
  var arm = /arm64|aarch64/i.test(ua);
  var FILES = { mac: 'Panoply-0.3.0-beta-macos-arm64.dmg', win: 'Panoply-0.3.0-beta-windows-' + (arm ? 'arm64' : 'x64') + '.msi', linux: 'Panoply-0.3.0-beta-linux-' + (arm ? 'arm64' : 'x64') + '.AppImage' };
  var NAMES = { mac: 'macOS', win: 'Windows', linux: 'Linux' };
  document.querySelectorAll('[data-dl]').forEach(function(a){ a.textContent = 'Download for ' + NAMES[os]; a.href = REL + FILES[os]; });
  if (os !== 'mac') document.querySelectorAll('.key')[0].textContent = 'Ctrl';

  document.querySelectorAll('.copybtn').forEach(function(btn){
    btn.addEventListener('click', function(){
      var el = document.getElementById(btn.dataset.copy);
      var text = el.dataset.full || el.textContent;
      function done(){ btn.textContent = 'Copied'; setTimeout(function(){ btn.textContent = 'Copy'; }, 1600); }
      function select(){ var r = document.createRange(); r.selectNodeContents(el); var s = getSelection(); s.removeAllRanges(); s.addRange(r); }
      try { navigator.clipboard.writeText(text).then(done, select); } catch (e) { select(); }
    });
  });

  /* ---------- the live command bar ---------- */
  var ICONS = {
    pdf: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>',
    img: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="2"/><circle cx="9" cy="9" r="1.6"/><path d="m20 15-4-4-8 8"/></svg>',
    vid: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="5" width="13" height="14" rx="2"/><path d="m16 10 5-3v10l-5-3"/></svg>',
    id: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="9" cy="11" r="2"/><path d="M14 10h4M14 14h4M6 16h6"/></svg>'
  };
  var CHECK = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9.5"/><path d="m8 12.5 2.7 2.7L16.5 9.5"/></svg>';
  function fi(k){ return '<span class="fi ' + k + '">' + ICONS[k] + '</span>'; }
  function row(k, name, desc, right){ return '<div class="row">' + fi(k) + '<b>' + name + '</b><span class="d">' + desc + '</span><span class="k">' + right + '</span></div>'; }

  var REST = '<div class="sec">Suggested</div>' +
    row('img', 'Resize to exact KB', 'Fit a photo to the exact pixels and KB a form or exam portal asks for.', 'Image') +
    row('pdf', 'Compress PDF', 'Make this PDF small enough to email or upload.', 'PDF') +
    row('id', 'Print sheet', 'Put several copies of this photo on one sheet for the print shop.', 'ID and forms') +
    '<div class="sec">Recent</div>' +
    row('img', 'Resize to exact KB', 'signature.jpg', '2 min ago');

  var SCENES = [
    { k: 'pdf', file: 'Lease.pdf', size: '4.2 MB', ask: 'small enough to email',
      tool: 'Compress PDF', desc: 'Make this PDF small enough to email or upload.',
      opts: [['Target', 'under size'], ['Stay under', '2 MB']], est: '4.2 MB to at most 2 MB',
      also: ['Split PDF', 'Cut this PDF into parts by page ranges or every N pages.', 'PDF'],
      more: [['Merge PDF', 'Put these PDFs in one file.', 'PDF'], ['OCR', 'Make this scan searchable and selectable.', 'PDF'], ['PDF to images', 'Turn each page of this PDF into a PNG or JPG picture.', 'PDF']],
      head: '4.2 MB to 880 KB, 79% smaller', before: '4.2 MB', after: '880 KB', out: 'Lease (compressed).pdf' },
    { k: 'img', file: 'signature.png', size: '67 KB', ask: 'under 50 kb',
      tool: 'Compress image', desc: 'Make this image smaller without it looking worse, or fit it under a size.',
      opts: [['Target', 'under size'], ['Stay under', '50 KB'], ['Format', 'keep']], est: '67 KB to at most 50 KB',
      also: ['Resize to exact KB', 'Fit a photo to the exact pixels and KB a form or exam portal asks for.', 'Image'],
      more: [['Remove background', 'Cut the subject out of the picture.', 'Image'], ['Convert image', 'Change the format, like HEIC to JPG.', 'Image'], ['Upscale', 'Make a small picture four times larger.', 'Image']],
      head: '67 KB to 43 KB, 35% smaller', before: '67 KB', after: '43 KB', out: 'signature (compressed).png' },
    { k: 'vid', file: 'keynote.mov', size: '4.6 GB', ask: 'cut the first 10 minutes',
      tool: 'Trim video', desc: 'Cut the start or the end, at keyframes, without re-encoding.',
      opts: [['Start', '10:00'], ['End', 'keep']], est: 'about a second',
      also: ['Compress video', 'Fit a video under a size or an upload limit.', 'Video'],
      more: [['Extract audio', 'Save the sound of this video on its own.', 'Video'], ['Convert video', 'Change the format so it plays anywhere.', 'Video'], ['Even loudness', 'Make quiet and loud parts sit at one level.', 'Video']],
      head: 'Trimmed in 0.96 s', before: '73 min', after: '63 min', out: 'keynote (trimmed).mov' }
  ];

  var bar = document.getElementById('bar');
  var $ = function(s){ return bar.querySelector(s); };
  var views = { list: $('.view.list'), match: $('.view.match'), run: $('.view.run'), done: $('.view.done') };
  var typed = $('.typed'), ph = $('.ph'), chip = $('.fchip');
  var token = 0, scene = 0, playing = false;

  function show(name){ for (var k in views) views[k].classList.toggle('on', k === name); }
  function wait(ms, t){ return new Promise(function(res, rej){ setTimeout(function(){ t === token ? res() : rej(); }, ms); }); }
  function fileList(s){
    var forK = s.k === 'pdf' ? 'For this PDF document, best first' : s.k === 'img' ? 'For this picture, best first' : 'For this video, best first';
    return '<div class="sec">' + forK + '</div>' + row(s.k, s.tool, s.desc, '&#8629;').replace('class="row"', 'class="row sel"') +
      row(s.k, s.also[0], s.also[1], s.also[2]) + s.more.map(function(m){ return row(s.k, m[0], m[1], m[2]); }).join('');
  }
  function matchView(s){
    var o = s.opts.map(function(p){ return '<span>' + p[0] + '<b>' + p[1] + '</b></span>'; }).join('');
    return '<div class="sec">Best match</div><div class="row sel">' + fi(s.k) + '<b>' + s.tool + '</b><span class="d">' + s.desc + '</span><span class="kbd">&#8629;</span>' +
      '<div class="opts">' + o + '</div><span class="est">' + s.est + '</span></div>' +
      '<div class="sec">Also matches</div>' + row(s.k, s.also[0], s.also[1], s.also[2]) + row(s.k, s.more[0][0], s.more[0][1], s.more[0][2]);
  }
  function doneView(s){
    return '<h4>' + CHECK + s.head + '</h4><div class="ba"><div><small>Before</small><strong>' + s.before + '</strong></div><span>&#8594;</span><div class="after"><small>After</small><strong>' + s.after + '</strong></div></div>' +
      '<div class="out">Written next to the original<br><span>' + s.out + '</span></div>' +
      '<div class="acts"><span>Undo</span><span>Show in folder</span><span class="go">Open</span></div>';
  }
  var narrow = function(){ return innerWidth <= 734; };
  function reset(){
    typed.textContent = ''; ph.textContent = narrow() ? 'Search tools or drop a file' : 'Search 31 tools and your files, or drop anything'; ph.hidden = false;
    chip.classList.remove('on'); views.list.innerHTML = REST; show('list');
  }
  async function play(){
    var t = ++token;
    try {
      for (;;) {
        var s = SCENES[scene];
        reset();
        await wait(1100, t);
        chip.querySelector('.fi').className = 'fi ' + s.k; chip.querySelector('.fi').innerHTML = ICONS[s.k];
        chip.querySelector('.fn').textContent = s.file; chip.querySelector('.fs').textContent = s.size;
        chip.classList.add('on'); ph.textContent = narrow() ? 'What should happen?' : 'What do you want to do with this file?';
        views.list.innerHTML = fileList(s);
        await wait(1000, t);
        ph.hidden = true;
        for (var i = 1; i <= s.ask.length; i++) { typed.textContent = s.ask.slice(0, i); await wait(48 + Math.random() * 40, t); }
        await wait(260, t);
        views.match.innerHTML = matchView(s); show('match');
        await wait(1500, t);
        var sel = views.match.querySelector('.row.sel'); sel.classList.add('hit');
        await wait(220, t);
        $('.rt').textContent = 'Running ' + s.tool; $('.rf').textContent = s.file; show('run');
        var bar_ = $('.prog i'), pct = $('.rp'), t0 = performance.now(), dur = s.k === 'vid' ? 700 : 1100;
        for (;;) { var k = Math.min(1, (performance.now() - t0) / dur); bar_.style.transform = 'scaleX(' + k + ')'; pct.textContent = Math.round(k * 100) + '%'; if (k >= 1) break; await wait(30, t); }
        await wait(180, t);
        views.done.innerHTML = doneView(s); show('done');
        await wait(2600, t);
        $('.bar-body').classList.add('fade'); chip.classList.remove('on');
        await wait(240, t);
        $('.bar-body').classList.remove('fade');
        scene = (scene + 1) % SCENES.length;
      }
    } catch (e) { /* a newer run took over */ }
  }
  function startBar(){ if (!playing && !still) { playing = true; play(); } }
  function stopBar(){ if (playing) { playing = false; token++; } }
  if (still) { var s0 = SCENES[1]; chip.classList.add('on'); chip.querySelector('.fi').className = 'fi img'; chip.querySelector('.fi').innerHTML = ICONS.img; chip.querySelector('.fn').textContent = s0.file; chip.querySelector('.fs').textContent = s0.size; ph.hidden = true; typed.textContent = s0.ask; views.match.innerHTML = matchView(s0); show('match'); }
  else views.list.innerHTML = REST;

  /* ---------- statement words ---------- */
  var para = document.querySelector('[data-words]'), words = [], lit = -1;
  if (para) {
    para.innerHTML = para.textContent.trim().split(/\s+/).map(function(w){ return '<span class="w">' + w + '</span>'; }).join(' ');
    words = [].slice.call(para.querySelectorAll('.w'));
  }

  /* ---------- scroll-linked moments ---------- */
  var nav = document.querySelector('.nav'), hero = document.querySelector('.hero'), how = document.querySelector('.how');
  var stepsEl = [].slice.call(document.querySelectorAll('.step'));
  var appShot = document.querySelector('.app-shot'), curStep = -1;
  var bar2 = document.getElementById('bar2');
  var b2 = function(q){ return bar2.querySelector(q); };
  var b2v = { list: b2('.view.list'), match: b2('.view.match'), done: b2('.view.done') };
  var S1 = SCENES[1];
  b2v.list.innerHTML = REST; b2v.match.innerHTML = matchView(S1); b2v.done.innerHTML = doneView(S1);
  b2('.fchip .fi').className = 'fi img'; b2('.fchip .fi').innerHTML = ICONS.img;
  b2('.fchip .fn').textContent = S1.file; b2('.fchip .fs').textContent = S1.size;
  b2('.run').remove();
  function setStep(st){
    if (st === curStep) return; curStep = st;
    stepsEl.forEach(function(e, i){ e.classList.toggle('on', i === st); });
    var name = st === 0 ? 'list' : st === 1 ? 'match' : 'done';
    for (var k in b2v) b2v[k].classList.toggle('on', k === name);
    b2('.fchip').classList.toggle('on', st > 0);
    b2('.typed').textContent = st > 0 ? S1.ask : '';
    b2('.ph').hidden = st > 0;
    b2('.ph').textContent = 'Search tools or drop a file';
  }
  setStep(0);
  function clamp(v){ return v < 0 ? 0 : v > 1 ? 1 : v; }
  function wide(){ return innerWidth > 734 && !still; }
  function update(){
    var vh = innerHeight;
    nav.classList.toggle('solid', scrollY > 8);
    if (wide()) {
      var r = hero.getBoundingClientRect();
      var hp0 = clamp(-r.top / (r.height - vh));
      hero.style.setProperty('--pc', Math.pow(hp0, 1.5).toFixed(4));
      hero.style.setProperty('--pa', (1 - Math.pow(1 - hp0, 2)).toFixed(4));
      var h = how.getBoundingClientRect(), hp = clamp(-h.top / (h.height - vh));
      setStep(hp < .36 ? 0 : hp < .7 ? 1 : 2);
      var a = appShot.getBoundingClientRect();
      appShot.style.setProperty('--q', clamp((vh - a.top) / (vh * .75)).toFixed(4));
    } else { hero.style.removeProperty('--pc'); hero.style.removeProperty('--pa'); appShot.style.setProperty('--q', 1); }
    if (words.length) {
      var line = vh * .55, n = 0;
      if (still) n = words.length; else while (n < words.length && words[n].getBoundingClientRect().top < line) n++;
      if (n !== lit) { lit = n; words.forEach(function(w, i){ w.classList.toggle('lit', i < n); }); }
    }
  }
  addEventListener('scroll', update, { passive: true });
  addEventListener('resize', update);
  update();

  /* ---------- entrances and small demos ---------- */
  function count(el){
    var to = parseFloat(el.dataset.count), dec = (el.dataset.count.split('.')[1] || '').length;
    var unit = el.querySelector('small').outerHTML, t0 = performance.now();
    (function step(){
      var k = Math.min(1, (performance.now() - t0) / 1400), e = 1 - Math.pow(1 - k, 3);
      el.innerHTML = (to * e).toFixed(dec) + unit;
      if (k < 1) setTimeout(step, 16);
    })();
  }
  function typeTerm(){
    var code = document.getElementById('mcp'), full = code.dataset.full, lines = [].slice.call(document.querySelectorAll('.log div'));
    code.textContent = ''; var i = 0;
    (function tick(){
      code.textContent = full.slice(0, ++i);
      if (i < full.length) return setTimeout(tick, 32);
      lines.forEach(function(l, j){ setTimeout(function(){ l.classList.add('on'); }, 500 + j * 650); });
    })();
  }
  // Entrances: checked on scroll, so they work in every browser and in background tabs.
  root.classList.add('js');
  var pending = [].slice.call(document.querySelectorAll('.rv, .offline'));
  function enter(el){
    el.classList.add('in');
    var c = el.querySelector('[data-count]'); if (c && !still) count(c);
    if (el.classList.contains('agent-tile') && !still) typeTerm();
    if (el.classList.contains('offline')) setTimeout(function(){ var ok = el.querySelector('.ok'); ok.classList.add('fin'); ok.querySelector('.okt').innerHTML = 'passport.jpg resized, <em>now under 50 KB</em>'; }, still ? 0 : 2800);
  }
  function reveal(){
    if (!pending.length) return;
    var line = innerHeight * .9;
    pending = pending.filter(function(el){ if (el.getBoundingClientRect().top < line) { enter(el); return false; } return true; });
  }
  addEventListener('scroll', reveal, { passive: true });
  reveal();
  if (still) document.querySelectorAll('.log div').forEach(function(l){ l.classList.add('on'); });

  // The demos only run while they are on screen.
  var cyc = null;
  function demos(){
    var b = bar.getBoundingClientRect(), on = b.bottom > 0 && b.top < innerHeight;
    if (on && !document.hidden) startBar(); else stopBar();
    if (wide() && cyc) { clearInterval(cyc); cyc = null; }
    if (!wide() && !still) {
      var c = bar2.getBoundingClientRect(), on2 = c.bottom > 0 && c.top < innerHeight;
      if (on2 && !cyc) cyc = setInterval(function(){ setStep((curStep + 1) % 3); }, 2600);
      if (!on2 && cyc) { clearInterval(cyc); cyc = null; }
    }
  }
  addEventListener('scroll', demos, { passive: true });
  addEventListener('resize', demos);
  document.addEventListener('visibilitychange', demos);
  demos();
  window.__panoply = update;
})();
