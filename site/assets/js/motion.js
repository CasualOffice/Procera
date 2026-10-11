// Motion and interaction for the site. Vanilla JS, no dependencies.
// - With prefers-reduced-motion (or without JS) everything sits in its final state: the hero
//   story markup ships finished, counters show their real numbers, nothing loops.
// - Otherwise html.motion is set and elements reveal, counters count up, the hero story plays
//   in a loop while it is on screen (with a pause button), and the tour frame swaps screens.
// Tabs, the tour frame and the hash-chain demo work in both modes.
(function () {
  var root = document.documentElement;
  var reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var IO = 'IntersectionObserver' in window;
  var motion = !reduce && IO;

  function $all(sel, ctx) { return Array.prototype.slice.call((ctx || document).querySelectorAll(sel)); }
  function inView(el) { var r = el.getBoundingClientRect(); return r.top < innerHeight && r.bottom > 0; }

  // ------------------------------------------------------------ reveals and counters
  function count(el) {
    var to = parseInt(el.getAttribute('data-count'), 10), t0 = null, dur = 1300;
    if (!to) return;
    el.textContent = '0';
    function tick(t) {
      if (t0 === null) t0 = t;
      var p = Math.min((t - t0) / dur, 1), e = 1 - Math.pow(1 - p, 3);
      el.textContent = String(Math.round(to * e));
      if (p < 1) requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  }

  function reveals() {
    var els = $all('.rv, .draw, .timeline, .bar, [data-count]');
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        e.target.classList.add('in');
        if (e.target.hasAttribute('data-count')) count(e.target);
        io.unobserve(e.target);
      });
    }, { rootMargin: '0px 0px -8% 0px', threshold: 0.12 });
    // Anything already on screen is revealed before html.motion applies, so nothing flickers.
    els.forEach(function (el) {
      if (inView(el) && !el.hasAttribute('data-count')) el.classList.add('in');
      else io.observe(el);
    });
    root.classList.add('motion');
  }

  // ------------------------------------------------------------ hero story
  function story(el) {
    var DUR = [2600, 3000, 2400, 4200, 2400, 3200];
    var faces = $all('.faces li', el), ack = el.querySelector('[data-ack]'), btn = el.querySelector('.story-toggle');
    var step = -1, timer = null, faceTimer = null, paused = false, visible = false;

    function setStep(n) {
      step = n;
      el.setAttribute('data-step', String(n));
      for (var k = 0; k <= 5; k++) el.classList.toggle('s' + k, k <= n);
      if (n === 3) runFaces();
    }
    function setFaces(i) {
      faces.forEach(function (f, j) { f.classList.toggle('ok', j < i); });
      ack.textContent = String(i);
      el.style.setProperty('--p', String(i / faces.length * 100));
    }
    function runFaces() {
      var i = 0;
      clearInterval(faceTimer);
      faceTimer = setInterval(function () { setFaces(++i); if (i >= faces.length) clearInterval(faceTimer); }, 240);
    }
    function reset() {
      el.classList.add('reset');
      clearInterval(faceTimer);
      setStep(-1);
      setFaces(0);
      void el.offsetWidth;
      el.classList.remove('reset');
    }
    function next() {
      var n = step + 1;
      if (n > 5) { reset(); n = 0; }
      setStep(n);
      timer = setTimeout(next, DUR[n]);
    }
    function sync() {
      var run = visible && !paused && !document.hidden;
      if (run && !timer) timer = setTimeout(next, step < 0 ? 400 : 1200);
      if (!run && timer) { clearTimeout(timer); timer = null; }
      if (!run && step === 3) { clearInterval(faceTimer); setFaces(faces.length); }
    }

    reset();
    btn.hidden = false;
    btn.addEventListener('click', function () {
      paused = !paused;
      btn.setAttribute('aria-pressed', paused ? 'true' : 'false');
      btn.textContent = paused ? 'Play' : 'Pause';
      sync();
    });
    new IntersectionObserver(function (es) { visible = es[0].isIntersecting; sync(); }, { threshold: 0.25 }).observe(el);
    document.addEventListener('visibilitychange', sync);
  }

  // ------------------------------------------------------------ sticky tour frame
  function tour() {
    var frame = document.querySelector('.tour-frame');
    if (!frame || !IO) return;
    var layers = $all('.layer', frame), dots = $all('.tour-dots li', frame), url = frame.querySelector('[data-url]');
    var steps = $all('.tour-step');
    function show(i) {
      layers.forEach(function (l, j) { l.classList.toggle('on', j === i); });
      dots.forEach(function (d, j) { d.classList.toggle('on', j === i); });
      url.textContent = steps[i].getAttribute('data-url');
    }
    show(0);
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) show(+e.target.getAttribute('data-i')); });
    }, { rootMargin: '-45% 0px -45% 0px' });
    steps.forEach(function (s) { io.observe(s); });
  }

  // ------------------------------------------------------------ persona tabs (APG tabs, automatic activation)
  function tabs(box) {
    var list = $all('[role=tab]', box);
    function select(tab, focus) {
      list.forEach(function (t) {
        var on = t === tab, panel = document.getElementById(t.getAttribute('aria-controls'));
        t.setAttribute('aria-selected', on ? 'true' : 'false');
        t.tabIndex = on ? 0 : -1;
        panel.hidden = !on;
        panel.classList.toggle('on', on);
      });
      if (focus) tab.focus();
    }
    list.forEach(function (t, i) {
      t.addEventListener('click', function () { select(t, false); });
      t.addEventListener('keydown', function (e) {
        var n = { ArrowRight: i + 1, ArrowDown: i + 1, ArrowLeft: i - 1, ArrowUp: i - 1, Home: 0, End: list.length - 1 }[e.key];
        if (n === undefined) return;
        e.preventDefault();
        select(list[(n + list.length) % list.length], true);
      });
    });
  }

  // ------------------------------------------------------------ hash-chain demo
  function chain(box) {
    var blks = $all('.blk', box), tamper = box.querySelector('[data-tamper]'), restore = box.querySelector('[data-restore]');
    var status = box.querySelector('[data-status]'), who = blks[1].querySelector('[data-who]'), h = blks[1].querySelector('[data-h]');
    var orig = [who.textContent, h.textContent, status.innerHTML];
    tamper.addEventListener('click', function () {
      who.textContent = 'Someone else';
      h.textContent = 'd41f9e07';
      blks[1].classList.add('changed');
      blks[2].classList.add('broken');
      blks.slice(3).forEach(function (b) { b.classList.add('after'); });
      status.classList.add('bad');
      status.innerHTML = '<span class="ok" aria-hidden="true"></span>Verification fails at #1043: its previous hash no longer matches #1042';
      tamper.hidden = true; restore.hidden = false; restore.focus();
    });
    restore.addEventListener('click', function () {
      who.textContent = orig[0]; h.textContent = orig[1]; status.innerHTML = orig[2];
      blks.forEach(function (b) { b.classList.remove('changed', 'broken', 'after'); });
      status.classList.remove('bad');
      restore.hidden = true; tamper.hidden = false; tamper.focus();
    });
  }

  // ------------------------------------------------------------ features: highlight the pillar in view
  function pillarNav(nav) {
    var links = $all('a', nav), queued = false;
    var secs = links.map(function (a) { return document.querySelector(a.getAttribute('href')); });
    function update() {
      queued = false;
      var cur = -1;
      secs.forEach(function (s, i) { if (s && s.getBoundingClientRect().top < innerHeight * 0.35) cur = i; });
      links.forEach(function (a, i) { if (i === cur) a.setAttribute('aria-current', 'true'); else a.removeAttribute('aria-current'); });
    }
    addEventListener('scroll', function () { if (!queued) { queued = true; requestAnimationFrame(update); } }, { passive: true });
    update();
  }

  function init() {
    if (motion) {
      reveals();
      var s = document.querySelector('[data-story]');
      if (s) story(s);
    }
    tour();
    $all('[data-tabs]').forEach(tabs);
    $all('[data-chain]').forEach(chain);
    $all('.pillar-nav.sticky').forEach(pillarNav);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init); else init();
})();
