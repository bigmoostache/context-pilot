/**
 * Slide deck navigation — 2D grid (sections × slides).
 *
 * Left/Right : change section, preserve slide index (clamp to max).
 * Up/Down    : change slide within current section.
 */

const SECTION_NAMES = ['Overview', 'The WHYs', 'The HOWs', 'The WOWs'];

const SECTIONS = [
  // Section 0 — Main TOC
  [
    { path: 'slides/0-0.html', label: 'Overview', dark: true },
    { path: 'slides/0-1.html', label: 'Where it all started' },
    { path: 'slides/0-2.html', label: 'Where it is now' },
  ],
  // Section 1 — The WHYs
  [
    { path: 'slides/1-0.html', label: 'The WHYs', dark: true },
    { path: 'slides/1-2.html', label: "What's failing" },
    { path: 'slides/1-3.html', label: "What I'd Add" },
    { path: 'slides/1-1.html', label: 'Token Jewelry' },
  ],
  // Section 2 — The HOWs
  [
    { path: 'slides/2-0.html', label: 'The HOWs', dark: true },
    { path: 'slides/2-2.html', label: 'Architecture' },
    { path: 'slides/2-4.html', label: 'Feedbacks & Constraints' },
    { path: 'slides/2-1.html', label: 'Learnings: Pot-Pourri' },
  ],
  // Section 3 — The WOWs
  [
    { path: 'slides/3-0.html', label: 'The WOWs', dark: true },
    { path: 'slides/3-1.html', label: 'Showcase' },
    { path: 'slides/3-2.html', label: 'Self-Improving Loop' },
  ],
];

let sectionIdx = 0;
let slideIdx = 0;
let transitioning = false;

const cache = {};
const currentEl  = document.getElementById('slide-current');
const incomingEl = document.getElementById('slide-incoming');

/* ---------- Activate <script> tags injected via innerHTML ---------- */
function activateScripts(el) {
  el.querySelectorAll('script').forEach(old => {
    const s = document.createElement('script');
    s.textContent = old.textContent;
    old.parentNode.replaceChild(s, old);
  });
}

/* ---------- Bootstrap ---------- */
async function init() {
  const first = SECTIONS[0][0];
  const html = await fetchSlide(first.path);
  currentEl.innerHTML = html;
  activateScripts(currentEl);
  currentEl.className = 'slide center' + (first.dark ? ' slide-dark' : '');
  updateUI();
}

/* ---------- Fetch & cache ---------- */
async function fetchSlide(path) {
  if (cache[path]) return cache[path];
  const res = await fetch(path);
  const html = await res.text();
  cache[path] = html;
  return html;
}

/* ---------- Navigation ---------- */
function navigate(newSection, newSlide) {
  if (transitioning) return;
  if (newSection < 0 || newSection >= SECTIONS.length) return;

  const section = SECTIONS[newSection];
  // Clamp slide index to target section's bounds
  const clampedSlide = Math.min(newSlide, section.length - 1);
  if (clampedSlide < 0) return;

  // No-op?
  if (newSection === sectionIdx && clampedSlide === slideIdx) return;

  // Determine direction for animation
  let enterClass, exitClass;
  if (newSection > sectionIdx) {
    enterClass = 'enter-from-right';
    exitClass  = 'exit-left';
  } else if (newSection < sectionIdx) {
    enterClass = 'enter-from-left';
    exitClass  = 'exit-right';
  } else if (clampedSlide > slideIdx) {
    enterClass = 'enter-from-below';
    exitClass  = 'exit-up';
  } else {
    enterClass = 'enter-from-above';
    exitClass  = 'exit-down';
  }

  transitioning = true;
  const path = section[clampedSlide].path;

  fetchSlide(path).then(html => {
    const isDark = section[clampedSlide].dark;

    // Prepare incoming off-screen
    incomingEl.innerHTML = html;
    incomingEl.style.transition = 'none';
    incomingEl.className = 'slide ' + enterClass + (isDark ? ' slide-dark' : '');
    incomingEl.style.pointerEvents = 'auto';

    // Force reflow so the browser registers the start position
    void incomingEl.offsetWidth;

    // Enable transitions again
    incomingEl.style.transition = '';

    // Animate: incoming → center, current → exit
    requestAnimationFrame(() => {
      incomingEl.classList.remove(enterClass);
      incomingEl.classList.add('center');
      incomingEl.style.opacity = '1';

      currentEl.classList.remove('center');
      currentEl.classList.add(exitClass);
    });

    // After transition, swap roles (transitions OFF to avoid ghost animation)
    setTimeout(() => {
      currentEl.style.transition = 'none';
      currentEl.innerHTML = incomingEl.innerHTML;
      currentEl.className = 'slide center' + (isDark ? ' slide-dark' : '');
      currentEl.style.opacity = '1';
      activateScripts(currentEl);
      void currentEl.offsetWidth;          // force reflow before re-enabling
      currentEl.style.transition = '';

      incomingEl.style.transition = 'none';
      incomingEl.className = 'slide';
      incomingEl.style.opacity = '0';
      incomingEl.style.pointerEvents = 'none';
      void incomingEl.offsetWidth;
      incomingEl.style.transition = '';

      sectionIdx = newSection;
      slideIdx = clampedSlide;
      transitioning = false;
      updateUI();
    }, 650);
  });
}

/* ---------- Sidebar ---------- */
function buildSidebar() {
  const sidebar = document.getElementById('sidebar');
  sidebar.innerHTML = '';

  // Hide sidebar on title/TOC slides
  const currentSlide = SECTIONS[sectionIdx][slideIdx];
  if (currentSlide.dark) {
    sidebar.style.opacity = '0';
    sidebar.style.pointerEvents = 'none';
    return;
  }
  sidebar.style.opacity = '1';
  sidebar.style.pointerEvents = 'auto';

  // Section label
  const label = document.createElement('div');
  label.className = 'sidebar-label';
  label.textContent = SECTION_NAMES[sectionIdx];
  sidebar.appendChild(label);

  // Current section's slides only
  const slides = SECTIONS[sectionIdx];
  slides.forEach((slide, sli) => {
    if (slide.dark) return; // skip TOC slides
    const item = document.createElement('div');
    item.className = 'sidebar-item' + (sli === slideIdx ? ' active' : '');

    const pip = document.createElement('div');
    pip.className = 'sidebar-pip';

    const text = document.createElement('span');
    text.className = 'sidebar-item-label';
    text.textContent = slide.label;

    item.appendChild(pip);
    item.appendChild(text);
    item.addEventListener('click', () => navigate(sectionIdx, sli));
    sidebar.appendChild(item);
  });
}

/* ---------- Wire TOC clicks ---------- */
function wireTocClicks() {
  // Within-section navigation (data-slide)
  document.querySelectorAll('.toc-item[data-slide]').forEach(el => {
    el.addEventListener('click', () => {
      const target = parseInt(el.dataset.slide, 10);
      navigate(sectionIdx, target);
    });
  });
  // Cross-section navigation (data-section) — matches both .toc-item and .toc-col
  document.querySelectorAll('[data-section]').forEach(el => {
    el.addEventListener('click', () => {
      const target = parseInt(el.dataset.section, 10);
      navigate(target, 0);
    });
  });
}

/* ---------- UI indicators ---------- */
function updateUI() {
  buildSidebar();
  wireTocClicks();

  // Position
  document.getElementById('position').textContent =
    `${sectionIdx + 1}.${slideIdx + 1}`;
}

/* ---------- Keyboard ---------- */
document.addEventListener('keydown', (e) => {
  switch (e.key) {
    case 'ArrowRight': navigate(sectionIdx + 1, slideIdx);     break;
    case 'ArrowLeft':  navigate(sectionIdx - 1, slideIdx);     break;
    case 'ArrowDown':
      if (slideIdx >= SECTIONS[sectionIdx].length - 1) {
        navigate(sectionIdx + 1, 0);
      } else {
        navigate(sectionIdx, slideIdx + 1);
      }
      break;
    case 'ArrowUp':    navigate(sectionIdx, slideIdx - 1);     break;
  }
});

/* ---------- Persistent timer (survives reloads, click to reset) ---------- */
(function() {
  const STORAGE_KEY = 'slide_timer_start';
  const timerEl = document.getElementById('timer');

  if (!localStorage.getItem(STORAGE_KEY)) {
    localStorage.setItem(STORAGE_KEY, Date.now().toString());
  }

  function pad(n) { return n < 10 ? '0' + n : '' + n; }

  function updateTimer() {
    const start = parseInt(localStorage.getItem(STORAGE_KEY), 10);
    const elapsed = Math.floor((Date.now() - start) / 1000);
    const h = Math.floor(elapsed / 3600);
    const m = Math.floor((elapsed % 3600) / 60);
    const s = elapsed % 60;
    timerEl.textContent = h > 0
      ? pad(h) + ':' + pad(m) + ':' + pad(s)
      : pad(m) + ':' + pad(s);
  }

  timerEl.addEventListener('click', () => {
    localStorage.setItem(STORAGE_KEY, Date.now().toString());
    updateTimer();
  });

  updateTimer();
  setInterval(updateTimer, 1000);
})();

/* ---------- Go ---------- */
init();
