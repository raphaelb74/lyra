import './styles/tokens.css';
import './styles/base.css';
import './styles/components.css';
import App from './App.svelte';
import { mount } from 'svelte';

const target = document.getElementById('app');

if (!target) throw new Error('Missing #app element in index.html');

mount(App, { target });
