  // Apply saved appearance before first paint (no flash).
  try {
    const a = JSON.parse(localStorage.getItem('companion-ui') || '{}');
    const r = document.documentElement;
    r.dataset.theme = ['light', 'dark', 'system'].includes(a.theme) ? a.theme : 'system';
    r.dataset.density = a.density === 'comfortable' ? 'comfortable' : 'compact';
    r.dataset.font = ['system', 'segoe', 'mono', 'serif', 'verdana'].includes(a.font) ? a.font : 'system';
  } catch { /* defaults */ }
