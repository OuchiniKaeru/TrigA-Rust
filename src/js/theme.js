export const THEMES = [
  'light-blue',
  'light',
  'dark',
  'light-orange',
  'light-pink',
  'dark-tokyo',
];

export const DEFAULT_THEME = 'light-blue';

export function normalizeTheme(value) {
  return THEMES.includes(value) ? value : DEFAULT_THEME;
}

export function applyTheme(name) {
  const theme = normalizeTheme(name);
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem('triga-theme', theme);
  } catch {
    // Private mode etc: theme still applies for this session.
  }
  return theme;
}

export function loadTheme() {
  let saved = null;
  try {
    saved = localStorage.getItem('triga-theme');
  } catch {
    saved = null;
  }
  return applyTheme(saved);
}
