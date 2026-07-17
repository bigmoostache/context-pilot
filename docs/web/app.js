/* Context Pilot landing — fleet render, hero typewriter, reveal, contact form.
   No dependencies. Degrades to a static, readable page without JS or motion. */
(function () {
  'use strict';

  var reduceMotion = window.matchMedia &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  function esc(str) {
    return String(str).replace(/[&<>"]/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c];
    });
  }

  /* ── Fleet: a team of agents, one per project ─────────── */
  var fleet = [
    { realm: '~/support-triage',  status: 'live', line: 'flagging recurring issues', active: true },
    { realm: '~/q3-report',       status: 'turn', line: 'draft ready for your review' },
    { realm: '~/market-research', status: 'idle', line: 'sources gathered · resting' },
    { realm: '~/data-cleanup',    status: 'live', line: 'reconciling records in the background' },
    { realm: '~/onboarding-docs', status: 'idle', line: 'published 2m ago' },
    { realm: '~/vendor-contracts',status: 'turn', line: 'needs a decision from you' }
  ];

  function statusDot(s) {
    if (s === 'live') return '<span class="dot dot-live ' + (reduceMotion ? '' : 'pulse') + '"></span>';
    if (s === 'turn') return '<span class="dot dot-turn"></span>';
    return '<span class="dot"></span>';
  }

  var fleetEl = document.getElementById('fleet');
  if (fleetEl) {
    fleetEl.innerHTML = fleet.map(function (a) {
      return '<div class="tile' + (a.active ? ' is-active' : '') + '">' +
        '<div class="tile-top">' + statusDot(a.status) +
        '<span class="realm">' + esc(a.realm) + '</span></div>' +
        '<div class="tile-line">' + esc(a.line) + '</div>' +
      '</div>';
    }).join('');
  }

  /* ── "Context overflow" rows (left comparison card) ───── */
  var ctxBad = document.getElementById('ctx-bad');
  if (ctxBad) {
    var rows = '';
    for (var r = 0; r < 11; r++) {
      rows += '<div class="ctx-row" style="opacity:' + (1 - r * 0.06).toFixed(2) + '"></div>';
    }
    ctxBad.innerHTML = rows + '<span class="ctx-overflow">⚠ too much to hold</span>';
  }

  /* ── Hero terminal typewriter (a delegated task) ──────── */
  var script = [
    { cls: 'prompt', text: '› ', follow: 'user', followText: 'flag the recurring problems in last month\'s support tickets' },
    { cls: 'muted',  text: 'context-pilot · on it' },
    { cls: 'tool',   text: '⚙ read   tickets/2026-06  (214 items)' },
    { cls: 'tool',   text: '⚙ search common themes' },
    { cls: 'asst',   text: 'Three issues account for most of the' },
    { cls: 'asst',   text: 'volume — writing up the breakdown…' },
    { cls: 'tool',   text: '⚙ write  findings.md' },
    { cls: 'ok',     text: '✓ done · ready for your review' }
  ];

  var body = document.getElementById('term-body');

  function renderStatic() {
    if (!body) return;
    body.innerHTML = script.map(function (l) {
      if (l.follow) {
        return '<div class="term-line"><span class="' + l.cls + '">' + esc(l.text) +
          '</span><span class="user">' + esc(l.followText) + '</span></div>';
      }
      return '<div class="term-line"><span class="' + l.cls + '">' + esc(l.text) + '</span></div>';
    }).join('') + '<div class="term-line"><span class="prompt">› </span><span class="caret"></span></div>';
  }

  function typeInto(el, text, speed, done) {
    var i = 0;
    (function step() {
      el.textContent = text.slice(0, i);
      if (i++ <= text.length) { window.setTimeout(step, speed); }
      else { done(); }
    })();
  }

  function typewriter() {
    if (!body) return;
    body.innerHTML = '';
    var li = 0;

    function typeLine() {
      if (li >= script.length) {
        var done = document.createElement('div');
        done.className = 'term-line';
        done.innerHTML = '<span class="prompt">› </span><span class="caret"></span>';
        body.appendChild(done);
        return;
      }
      var l = script[li];
      var lineEl = document.createElement('div');
      lineEl.className = 'term-line';
      var head = document.createElement('span');
      head.className = l.cls;
      lineEl.appendChild(head);
      body.appendChild(lineEl);

      if (l.follow) {
        head.textContent = l.text;
        var tail = document.createElement('span');
        tail.className = l.follow;
        lineEl.appendChild(tail);
        typeInto(tail, l.followText, 26, next);
      } else if (l.cls === 'user' || l.cls === 'asst') {
        typeInto(head, l.text, 16, next);
      } else {
        head.textContent = l.text;
        window.setTimeout(next, 300);
      }
    }

    function next() { li++; typeLine(); }
    typeLine();
  }

  if (body) {
    if (reduceMotion) {
      renderStatic();
    } else {
      var started = false;
      var start = function () { if (!started) { started = true; typewriter(); } };
      if ('IntersectionObserver' in window) {
        var io = new IntersectionObserver(function (entries) {
          if (entries[0].isIntersecting) { start(); io.disconnect(); }
        }, { threshold: 0.4 });
        io.observe(body);
      } else {
        start();
      }
      window.setTimeout(start, 1200); // safety net
    }
  }

  /* ── Scroll reveal ────────────────────────────────────── */
  var revealTargets = document.querySelectorAll('.band-head, .step, .ctx, .proof-inner, .sovereign-inner, .appliance, .contact');
  if (!reduceMotion && 'IntersectionObserver' in window) {
    revealTargets.forEach(function (el) { el.classList.add('reveal'); });
    var ro = new IntersectionObserver(function (entries, obs) {
      entries.forEach(function (e) {
        if (e.isIntersecting) { e.target.classList.add('in'); obs.unobserve(e.target); }
      });
    }, { threshold: 0.12 });
    revealTargets.forEach(function (el) { ro.observe(el); });
  }

  /* ── Contact form ─────────────────────────────────────── */
  var form = document.getElementById('contact-form');
  var result = document.getElementById('form-result');

  if (form) {
    form.addEventListener('submit', function (ev) {
      ev.preventDefault();

      var name = form.elements.name;
      var email = form.elements.email;
      var message = form.elements.message;
      var ok = true;

      [name, email, message].forEach(function (f) {
        var field = f.closest('.field');
        var valid = f.value.trim() !== '' &&
          (f.type !== 'email' || /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(f.value));
        if (!valid) { ok = false; if (field) field.classList.add('invalid'); }
        else if (field) { field.classList.remove('invalid'); }
      });

      if (!ok) {
        if (result) {
          result.hidden = false;
          result.innerHTML = '<span class="accent">✗ check the highlighted fields</span> — name, a valid email, and a message are required.';
        }
        return;
      }

      var who = name.value.trim();
      var addr = email.value.trim();

      if (result) {
        result.hidden = false;
        result.innerHTML =
          '<span class="ok">✓ message queued</span>\n' +
          '<span class="muted">context-pilot › thanks, </span>' +
          '<span class="accent">' + esc(who) + '</span>\n' +
          '<span class="muted">we\'ll reply to </span><span class="accent">' + esc(addr) + '</span>';
      }
      form.querySelector('button[type="submit"]').textContent = 'Sent ✓';
      window.setTimeout(function () { if (result) result.scrollIntoView({ block: 'nearest' }); }, 0);
    });
  }
})();
