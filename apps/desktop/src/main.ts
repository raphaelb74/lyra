import './styles/tokens.css';
import './styles/base.css';
import './styles/components.css';
import App from './App.svelte';
import { isTyping } from './lib/shortcuts';
import { mount } from 'svelte';

const target = document.getElementById('app');

if (!target) throw new Error('Missing #app element in index.html');

// Focus rings follow the keyboard: a click leaves none (styles/base.css).
const root = document.documentElement;

root.dataset.input = 'pointer';

addEventListener(
  'pointerdown',
  () => {
    root.dataset.input = 'pointer';
  },
  true,
);

addEventListener(
  'keydown',
  (event) => {
    if (event.key === 'Tab' || event.key.startsWith('Arrow')) {
      root.dataset.input = 'keyboard';
    }
  },
  true,
);

// No webview menu (Reload, Inspect) outside text fields: Lyra opens its own
// menus where it has one (lib/menu.svelte.ts).
addEventListener('contextmenu', (event) => {
  if (!isTyping(event.target)) event.preventDefault();
});

mount(App, { target });
