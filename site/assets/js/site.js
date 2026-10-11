// Theme toggle. The page follows prefers-color-scheme until the visitor picks a theme;
// the choice is kept in localStorage. Screenshots switch with it: each <picture> has a
// dark <source> whose media query we pin to "all" or "not all" once a theme is chosen.
(function () {
  var root = document.documentElement;
  var mq = window.matchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null;

  function current() {
    var t = root.getAttribute('data-theme');
    if (t === 'light' || t === 'dark') return t;
    return mq && mq.matches ? 'dark' : 'light';
  }

  function syncPictures() {
    var t = root.getAttribute('data-theme');
    var sources = document.querySelectorAll('source[data-dark]');
    for (var i = 0; i < sources.length; i++) {
      sources[i].media = t === 'dark' ? 'all' : t === 'light' ? 'not all' : '(prefers-color-scheme: dark)';
    }
  }

  function syncButton() {
    var b = document.querySelector('.theme-toggle');
    if (!b) return;
    var dark = current() === 'dark';
    b.setAttribute('aria-pressed', dark ? 'true' : 'false');
    b.setAttribute('aria-label', dark ? 'Switch to light theme' : 'Switch to dark theme');
  }

  document.addEventListener('DOMContentLoaded', function () {
    syncPictures();
    syncButton();
    var b = document.querySelector('.theme-toggle');
    if (b) b.addEventListener('click', function () {
      var next = current() === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-theme', next);
      try { localStorage.setItem('procera-theme', next); } catch (e) { /* private mode */ }
      syncPictures();
      syncButton();
    });
    if (mq && mq.addEventListener) mq.addEventListener('change', syncButton);

    // Close the mobile menu after choosing a link or pressing Escape.
    var menu = document.querySelector('.menu');
    if (menu) {
      menu.addEventListener('keydown', function (e) {
        if (e.key === 'Escape' && menu.open) { menu.open = false; menu.querySelector('summary').focus(); }
      });
    }
  });
})();
