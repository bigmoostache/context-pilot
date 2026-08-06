/* Page backdrop — a live Voronoi partition.
 *
 * Why a Voronoi and not a particle constellation: a Voronoi is a map of
 * territories, one owner per cell, no overlap. That is the product's own
 * model — one agent per project, each with its own folder.
 *
 * The cursor does not select or light anything. It only deforms the field:
 * sites drift away from it and spring back, so the partition redraws itself
 * around the pointer. Reaction, not highlighting.
 *
 * Cells are built by half-plane clipping: start each site with the canvas
 * rectangle and clip it by the perpendicular bisector against every other
 * site. With a few dozen sites that is a couple of thousand cheap vertex
 * operations per frame — no triangulation, no dependency.
 */

const TAU = Math.PI * 2;

/** Clip `poly` to the half-plane of points p where (p - m) · d <= 0. */
function clipHalfPlane(poly, mx, my, dx, dy) {
  const out = [];
  const n = poly.length;

  for (let i = 0; i < n; i++) {
    const ax = poly[i][0];
    const ay = poly[i][1];
    const bx = poly[(i + 1) % n][0];
    const by = poly[(i + 1) % n][1];

    const fa = (ax - mx) * dx + (ay - my) * dy;
    const fb = (bx - mx) * dx + (by - my) * dy;

    if (fa <= 0) out.push([ax, ay]);
    if ((fa <= 0) !== (fb <= 0)) {
      const t = fa / (fa - fb);
      out.push([ax + (bx - ax) * t, ay + (by - ay) * t]);
    }
  }
  return out;
}

export function mountVeil(canvas, opts = {}) {
  if (!canvas || !canvas.getContext) return;

  const reduceMotion =
    window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // Deliberately faint: a backdrop you half-notice. If it competes with the
  // type, it's wrong. One colour throughout — no cell is ever singled out.
  const ctx = canvas.getContext('2d', { alpha: true });
  const edge = opts.edge || 'rgba(150,150,175,0.09)';
  const seedDot = opts.seedDot || 'rgba(150,150,175,0.14)';

  let w = 0;
  let h = 0;
  let dpr = 1;
  let sites = [];
  let raf = null;
  let running = false;

  const pointer = { x: -9999, y: -9999, live: false };

  function seed() {
    // Density scales with area so the cells stay a similar size on any screen.
    const target = Math.round(Math.min(30, Math.max(10, (w * h) / 42000)));
    sites = [];
    for (let i = 0; i < target; i++) {
      // Deterministic-ish spread: jittered grid beats pure random clumping.
      const cols = Math.ceil(Math.sqrt(target * (w / Math.max(h, 1))));
      const rows = Math.ceil(target / cols);
      const cx = ((i % cols) + 0.5) * (w / cols);
      const cy = (Math.floor(i / cols) + 0.5) * (h / rows);
      const jx = (Math.random() - 0.5) * (w / cols) * 0.85;
      const jy = (Math.random() - 0.5) * (h / rows) * 0.85;
      const a = Math.random() * TAU;
      const sp = 0.045 + Math.random() * 0.055;
      sites.push({
        x: cx + jx,
        y: cy + jy,
        hx: cx + jx, // home, so drift and repulsion always have something to return to
        hy: cy + jy,
        vx: Math.cos(a) * sp,
        vy: Math.sin(a) * sp,
      });
    }
  }

  function resize() {
    const rect = canvas.getBoundingClientRect();
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = Math.max(1, Math.round(rect.width));
    h = Math.max(1, Math.round(rect.height));
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    seed();
  }

  function step() {
    for (const s of sites) {
      s.x += s.vx;
      s.y += s.vy;

      // Spring back home so the field never drifts apart or piles up.
      s.vx += (s.hx - s.x) * 0.00035;
      s.vy += (s.hy - s.y) * 0.00035;

      if (pointer.live) {
        const dx = s.x - pointer.x;
        const dy = s.y - pointer.y;
        const d2 = dx * dx + dy * dy;
        const R = 150;
        if (d2 < R * R && d2 > 0.01) {
          // Soft push: cells breathe around the cursor rather than scatter.
          const d = Math.sqrt(d2);
          const f = (1 - d / R) * 0.18;
          s.vx += (dx / d) * f;
          s.vy += (dy / d) * f;
        }
      }

      s.vx *= 0.94;
      s.vy *= 0.94;
    }
  }

  function draw() {
    ctx.clearRect(0, 0, w, h);

    const rect = [[0, 0], [w, 0], [w, h], [0, h]];

    ctx.lineJoin = 'round';
    ctx.lineWidth = 1;
    ctx.strokeStyle = edge;
    ctx.fillStyle = seedDot;

    for (let i = 0; i < sites.length; i++) {
      let poly = rect;
      const si = sites[i];

      for (let j = 0; j < sites.length && poly.length; j++) {
        if (i === j) continue;
        const sj = sites[j];
        const dx = sj.x - si.x;
        const dy = sj.y - si.y;
        poly = clipHalfPlane(poly, (si.x + sj.x) / 2, (si.y + sj.y) / 2, dx, dy);
      }
      if (poly.length < 3) continue;

      ctx.beginPath();
      ctx.moveTo(poly[0][0], poly[0][1]);
      for (let k = 1; k < poly.length; k++) ctx.lineTo(poly[k][0], poly[k][1]);
      ctx.closePath();
      ctx.stroke();

      ctx.beginPath();
      ctx.arc(si.x, si.y, 1.2, 0, TAU);
      ctx.fill();
    }
  }

  function frame() {
    step();
    draw();
    raf = window.requestAnimationFrame(frame);
  }

  function start() {
    if (running || reduceMotion) return;
    running = true;
    raf = window.requestAnimationFrame(frame);
  }

  function stop() {
    running = false;
    if (raf) window.cancelAnimationFrame(raf);
    raf = null;
  }

  resize();
  draw();

  if (reduceMotion) return; // one static partition, no loop, no pointer

  // Only animate while the hero is actually on screen.
  if ('IntersectionObserver' in window) {
    const io = new IntersectionObserver(
      (entries) => { entries[0].isIntersecting ? start() : stop(); },
      { threshold: 0 }
    );
    io.observe(canvas);
  } else {
    start();
  }

  // Tracked on the window: the canvas is fixed and pointer-transparent, so it
  // never receives events itself, and the field should react anywhere on the
  // page — not only over one section.
  window.addEventListener('pointermove', (ev) => {
    if (ev.pointerType === 'touch') return;
    pointer.x = ev.clientX;
    pointer.y = ev.clientY;
    pointer.live = true;
  }, { passive: true });

  document.addEventListener('pointerleave', () => { pointer.live = false; }, { passive: true });

  window.addEventListener('resize', () => {
    resize();
    if (!running) draw();
  }, { passive: true });

  document.addEventListener('visibilitychange', () => {
    document.hidden ? stop() : start();
  });
}
