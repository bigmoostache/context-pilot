/* Daharness landing — the board, the model swap, scroll reveal.
   No dependencies. Without JS the page still reads; without motion it still works. */

import { mountVeil } from './veil.js';

const reduceMotion =
  window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

/* ── Page backdrop: a partition that reshapes itself around the cursor ── */
mountVeil(document.getElementById('veil'));

/* ── The board ───────────────────────────────────────────
   Six tasks from six different worlds, five different models,
   all at once. This is the pitch, not an illustration. */

const AGENTS = [
  { path: '~/support-triage',   line: 'grouping 214 tickets into three recurring issues', model: 'claude-opus-5',   state: 'work' },
  { path: '~/lease-review',     line: 'cross-checking 40 leases against the new clause',  model: 'deepseek-v4-pro', state: 'work' },
  { path: '~/etl-reconcile',    line: '1.2M rows matched · 3 mismatches left',            model: 'llama-3.3-70b',   state: 'work' },
  { path: '~/q3-report',        line: 'draft ready — needs your call on the forecast',    model: 'minimax-m2.7',    state: 'turn' },
  { path: '~/vendor-contracts', line: 'summarising the renewal terms',                    model: 'grok-4-1-fast',   state: 'work' },
  { path: '~/onboarding-docs',  line: 'published 2 min ago',                              model: 'claude-opus-5',   state: 'idle' },
];

const DOT = { work: 'dot dot-work', turn: 'dot dot-turn', idle: 'dot' };

function buildRow(a) {
  const row = document.createElement('div');
  row.className = 'row' + (a.state === 'turn' ? ' row-turn' : '');

  const dot = document.createElement('i');
  dot.className = DOT[a.state];
  dot.setAttribute('aria-hidden', 'true');

  const path = document.createElement('span');
  path.className = 'row-path';
  path.textContent = a.path;

  const line = document.createElement('span');
  line.className = 'row-line';
  line.textContent = a.line;

  const chip = document.createElement('span');
  chip.className = 'chip';
  chip.textContent = a.model;

  row.append(dot, path, line, chip);
  return row;
}

const rowsEl = document.getElementById('board-rows');
const statEl = document.getElementById('board-stat');

if (rowsEl) {
  const working = AGENTS.filter((a) => a.state === 'work').length;

  AGENTS.forEach((a, i) => {
    const row = buildRow(a);
    if (!reduceMotion) {
      row.style.opacity = '0';
      row.style.transform = 'translateY(8px)';
      row.style.transition = 'opacity .45s cubic-bezier(.22,.68,.24,1), transform .45s cubic-bezier(.22,.68,.24,1)';
      window.setTimeout(() => {
        row.style.opacity = '1';
        row.style.transform = 'none';
      }, 140 + i * 110);
    }
    rowsEl.appendChild(row);
  });

  if (statEl) {
    const write = () => {
      statEl.innerHTML = `<b>${AGENTS.length}</b> agents · <b>${working}</b> working`;
    };
    if (reduceMotion) write();
    else window.setTimeout(write, 140 + AGENTS.length * 110);
  }
}

/* ── Rent vs keep ────────────────────────────────────────
   The chips cycle; the list beside them never moves. That
   contrast is the whole section. */

// Real model ids from the shipped provider roster. Nothing aspirational here —
// if it's on this list, you can pick it today.
const MODELS = [
  'claude-opus-5',
  'claude-sonnet-5',
  'grok-4-1-fast',
  'deepseek-v4-pro',
  'llama-3.3-70b',
  'minimax-m2.7',
];

const chipsEl = document.getElementById('swap-chips');

if (chipsEl) {
  const chips = MODELS.map((m, i) => {
    const el = document.createElement('span');
    el.className = 'chip ' + (i === 0 ? 'is-on' : 'is-off');
    el.textContent = m;
    chipsEl.appendChild(el);
    return el;
  });

  if (!reduceMotion && chips.length > 1) {
    let active = 0;
    const cycle = () => {
      chips[active].className = 'chip is-off';
      active = (active + 1) % chips.length;
      chips[active].className = 'chip is-on';
    };
    let timer = null;
    const io = new IntersectionObserver((entries) => {
      entries.forEach((e) => {
        if (e.isIntersecting && !timer) timer = window.setInterval(cycle, 1600);
        else if (!e.isIntersecting && timer) { window.clearInterval(timer); timer = null; }
      });
    }, { threshold: 0.3 });
    io.observe(chipsEl);
  }
}

/* ── Scroll reveal ───────────────────────────────────── */

if (!reduceMotion && 'IntersectionObserver' in window) {
  const targets = document.querySelectorAll(
    '.band-head, .work-row, .work-close, .swap, .pull, .ways, .box-hero, .box-grid, .box-spec, .trust-in, .steps, .start-cta'
  );
  targets.forEach((el) => el.classList.add('reveal'));

  const ro = new IntersectionObserver((entries, obs) => {
    entries.forEach((e) => {
      if (e.isIntersecting) { e.target.classList.add('in'); obs.unobserve(e.target); }
    });
  }, { threshold: 0.1, rootMargin: '0px 0px -40px 0px' });

  targets.forEach((el) => ro.observe(el));
}
