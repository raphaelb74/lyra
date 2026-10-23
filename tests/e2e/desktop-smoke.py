#!/usr/bin/env python3
"""Exercise the actual WebKitGTK window and Rust IPC through tauri-driver.

Launches the binary packaged by `tauri build --debug --bundles deb`
(scripts/check.sh). That copy embeds the frontend; target/debug/lyra-desktop
may have been overwritten by `tauri dev`, which expects a dev server.
Requires tauri-driver, WebKitWebDriver (webkit2gtk-driver), and a display
(xvfb-run in CI). Uses only the Python standard library. Never opens an
audio device.
"""
import base64
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
ARTIFACTS = ROOT / 'artifacts'
ARTIFACTS.mkdir(exist_ok=True)
def unused_port():
    with socket.socket() as connection:
        connection.bind(('127.0.0.1', 0))
        return connection.getsockname()[1]


def packaged_binary():
    matches = sorted(ROOT.glob('target/debug/bundle/deb/*/data/usr/bin/lyra-desktop'))
    if not matches:
        raise SystemExit('No packaged binary; run scripts/check.sh (tauri build --debug --bundles deb) first.')
    return matches[-1]


PORT = unused_port()
BASE = f'http://127.0.0.1:{PORT}'


def request(method, path, body=None, timeout=30):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, method=method,
                                 headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            value = json.load(response)['value']
    except urllib.error.HTTPError as error:
        raise RuntimeError(error.read().decode()) from error
    if isinstance(value, dict) and 'error' in value:
        raise RuntimeError(value)
    return value


def wait_for(fn, seconds=30):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            value = fn()
            if value:
                return value
        except (OSError, RuntimeError) as error:
            last = error
        time.sleep(0.25)
    raise RuntimeError(f'Timed out waiting for desktop: {last}')


def check_appearance(execute, prefix):
    """Exercise shared CSS in WebKit, including real pointer pseudo-states.

    Synthetic rows isolate the shared state cascade without adding music,
    changing the database or opening an audio device.
    """
    def theme(value):
        execute(f"document.querySelector('input[name=theme][value={value}]').click(); return true")
        wait_for(lambda: execute(f"return document.documentElement.dataset.theme === '{value}'"))

    theme('arctic-blue')
    execute("document.documentElement.dataset.appearanceReload = 'pending'; location.reload(); return true")
    wait_for(lambda: execute("""
        return document.documentElement.dataset.appearanceReload !== 'pending'
            && document.documentElement.dataset.theme === 'arctic-blue'
            && document.querySelector('[data-connected=true]') !== null;
    """))
    execute("document.querySelector('nav.end button').click(); return true")
    wait_for(lambda: execute("return document.querySelector('input[name=theme]') !== null"))

    execute("""
        const fixture = document.createElement('section');
        fixture.id = 'ui-states';
        fixture.style.cssText = `position:fixed; inset:64px 24px auto 80px;
            padding:24px; z-index:110; background:var(--bg);
            border:1px solid var(--line); border-radius:var(--radius-m);
            --columns:32px minmax(0,1fr) 80px;`;
        fixture.innerHTML = `
            <h2>Track interaction states</h2>
            <p class="muted">Neutral hover · slate selection · accent playback</p>
            <ol class="rows" style="margin:24px 0">
                <li><button id="ui-idle" class="row"><span>1</span><span>Hover this track</span><span class="meta">3:24</span></button></li>
                <li><button id="ui-selected" class="row" data-selected="true"><span>2</span><span>Selected track</span><span class="meta">4:16</span></button></li>
                <li><button id="ui-playing" class="row" aria-current="true"><span>3</span><span>Currently playing</span><span class="meta">5:08</span></button></li>
                <li><button id="ui-both" class="row" aria-current="true" data-selected="true"><span>4</span><span>Playing and selected</span><span class="meta">2:52</span></button></li>
            </ol>
            <div class="actions">
                <button id="ui-primary" class="primary">Primary action</button>
                <button id="ui-round" class="primary icon" aria-label="Primary transport">▶</button>
                <button>Secondary</button><button class="tertiary">Tertiary</button>
                <button class="danger">Delete</button><button class="primary" disabled>Disabled</button>
            </div>`;
        document.querySelector('.app').append(fixture);
        window.uiColour = colour => {
            const canvas = document.createElement('canvas');
            canvas.width = canvas.height = 1;
            const context = canvas.getContext('2d');
            context.fillStyle = colour;
            context.fillRect(0, 0, 1, 1);
            return Array.from(context.getImageData(0, 0, 1, 1).data);
        };
        return true;
    """)

    def snapshot():
        return execute("""
            const root = getComputedStyle(document.documentElement);
            const token = name => window.uiColour(root.getPropertyValue('--' + name));
            const row = id => {
                const element = document.getElementById('ui-' + id);
                const style = getComputedStyle(element);
                const mark = getComputedStyle(element, '::before');
                return { background: window.uiColour(style.backgroundColor),
                    colour: window.uiColour(style.color),
                    mark: mark.content === 'none' ? null
                        : { width: mark.width, colour: window.uiColour(mark.backgroundColor) },
                    meta: element.querySelector('.meta') ? window.uiColour(getComputedStyle(element.querySelector('.meta')).color) : null };
            };
            return { theme: document.documentElement.dataset.theme,
                idle: row('idle'), selected: row('selected'), playing: row('playing'),
                both: row('both'), primary: row('primary'), round: row('round'),
                hover: token('surface-hover'), selection: token('surface-selected'),
                selectedHover: token('surface-selected-hover'),
                playingBase: token('surface-playing'), playingHover: token('surface-playing-hover'),
                accent: token('accent'), accentHover: token('accent-light'), accentPressed: token('accent-mid') };
        """)

    def until_state(predicate):
        def ready():
            state = snapshot()
            return state if predicate(state) else None
        return wait_for(ready)

    def contrast(foreground, background):
        def luminance(rgba):
            linear = [(v / 255 / 12.92 if v / 255 <= 0.04045
                       else ((v / 255 + 0.055) / 1.055) ** 2.4) for v in rgba[:3]]
            return sum(c * w for c, w in zip(linear, (0.2126, 0.7152, 0.0722)))
        light, dark = sorted((luminance(foreground), luminance(background)), reverse=True)
        return (light + 0.05) / (dark + 0.05)

    def move(x, y, buttons=None):
        actions = [{'type': 'pointerMove', 'duration': 0, 'x': x, 'y': y}]
        actions.extend(buttons or [])
        request('POST', prefix + '/actions', {'actions': [{
            'type': 'pointer', 'id': 'ui-mouse', 'parameters': {'pointerType': 'mouse'},
            'actions': actions,
        }]})

    def hover(identifier, buttons=None):
        point = execute(f"""
            const box = document.getElementById('ui-{identifier}').getBoundingClientRect();
            return {{x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2)}};
        """)
        move(point['x'], point['y'], buttons)

    reports = []
    for selected_theme in ('champagne', 'arctic-blue'):
        theme(selected_theme)
        move(10, 10)
        state = until_state(lambda current:
                            current['playing']['background'] == current['playingBase']
                            and current['primary']['background'] == current['accent']
                            and current['round']['background'] == current['accent'])
        assert state['selected']['background'] == state['selection'], state
        assert state['both']['background'] == state['playingBase'], state
        assert len({tuple(state[key]) for key in ('hover', 'selection', 'playingBase')}) == 3, state
        # Playing rows carry a 3 px accent mark on the left; selection never does.
        assert state['playing']['mark'] == {'width': '3px', 'colour': state['accent']}, state
        assert state['selected']['mark'] is None, state
        assert state['both']['mark'] == state['playing']['mark'], state
        assert state['primary']['background'] == state['round']['background'] == state['accent'], state
        for identifier in ('selected', 'playing', 'both'):
            assert contrast(state[identifier]['colour'], state[identifier]['background']) >= 4.5, state
            assert contrast(state[identifier]['meta'], state[identifier]['background']) >= 4.5, state

        for identifier, expected in (('idle', 'hover'), ('selected', 'selectedHover'),
                                     ('playing', 'playingHover'), ('both', 'playingHover'),
                                     ('primary', 'accentHover')):
            hover(identifier)
            current = until_state(lambda current: current[identifier]['background'] == current[expected])
            assert contrast(current[identifier]['colour'], current[identifier]['background']) >= 4.5, current
            if current[identifier]['meta'] is not None:
                assert contrast(current[identifier]['meta'], current[identifier]['background']) >= 4.5, current

        hover('primary', [{'type': 'pointerDown', 'button': 0}])
        try:
            until_state(lambda current: current['primary']['background'] == current['accentPressed'])
        finally:
            hover('primary', [{'type': 'pointerUp', 'button': 0}])

        hover('idle')
        for width in (900, 1440):
            request('POST', prefix + '/window/rect', {'width': width, 'height': 800})
            hover('idle')
            until_state(lambda current: current['idle']['background'] == current['hover'])
            png = request('GET', prefix + '/screenshot')
            (ARTIFACTS / f'ui-{selected_theme}-{width}.png').write_bytes(base64.b64decode(png))
        reports.append(snapshot())

    (ARTIFACTS / 'ui-states.json').write_text(json.dumps(reports, indent=2) + '\n')
    execute("""
        document.getElementById('ui-states').remove();
        delete window.uiColour;
        return true;
    """)
    theme('champagne')
    print('PASS: persisted themes, distinct row states, playing+selected hover, shared primary buttons and pressed state')


def main():
    session = None
    with tempfile.TemporaryDirectory(prefix='lyra-desktop-test-') as profile, \
            (ARTIFACTS / 'desktop-driver.log').open('w') as log:
        command = ['tauri-driver', '--port', str(PORT), '--native-port', str(unused_port())]
        if native_driver := os.environ.get('LYRA_NATIVE_DRIVER'):
            command.extend(['--native-driver', native_driver])
        environment = os.environ.copy()
        for variable, folder in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                 ('XDG_CACHE_HOME', 'cache')):
            environment[variable] = str(Path(profile) / folder)
        driver = subprocess.Popen(command, stdout=log, stderr=log, env=environment)
        try:
            wait_for(lambda: request('GET', '/status'))
            # Session creation launches the app; a cold CI runner needs far longer
            # than a desktop for WebKitGTK's first start.
            session = request('POST', '/session', {'capabilities': {'alwaysMatch': {
                'tauri:options': {'application': str(packaged_binary())}
            }}}, timeout=120)['sessionId']
            prefix = f'/session/{session}'

            def execute(script):
                return request('POST', prefix + '/execute/sync', {'script': script, 'args': []})

            wait_for(lambda: execute("return document.querySelector('[data-connected=true]') !== null"))
            # list_devices crossed IPC (CI runners have no sound card: an empty list is fine).
            wait_for(lambda: execute("return document.querySelector('[data-testid=devices][data-loaded=true]') !== null"))
            # The engine's version and connection are under Settings > About.
            execute("document.querySelector('nav.end button').click(); return true")
            wait_for(lambda: execute("return document.querySelector('[data-testid=backend-status]') !== null"))
            state = execute("""
                return {
                    title: document.title,
                    devicesLoaded: document.querySelector('[data-testid=devices]').dataset.loaded,
                    player: document.querySelector('footer').textContent,
                    about: document.querySelector('.about').textContent,
                    playDisabled: document.querySelector('button[aria-label=Play]').disabled
                };
            """)
            assert state['title'] == 'Lyra — Music player', state
            assert state['devicesLoaded'] == 'true', state
            assert 'Nothing playing' in state['player'], state
            assert state['playDisabled'], state
            assert 'Lyra 0.1.0' in state['about'], state
            assert 'UNDA engine connected' in state['about'], state
            # Retry the connection from the visible UI as well as on initial mount.
            execute("document.querySelector('.about button').click(); return true")
            wait_for(lambda: execute("return document.querySelector('[data-connected=true]') !== null"))
            png = request('GET', prefix + '/screenshot')
            (ARTIFACTS / 'desktop.png').write_bytes(base64.b64decode(png))
            (ARTIFACTS / 'desktop-smoke.json').write_text(json.dumps(state, indent=2) + '\n')
            check_appearance(execute, prefix)
            print('PASS: native window, device list over IPC, idle player, initial and retried Rust IPC')
            print(json.dumps(state, indent=2))
        finally:
            try:
                if session:
                    request('DELETE', f'/session/{session}')
            finally:
                driver.terminate()
                try:
                    driver.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    driver.kill()
                    driver.wait()


if __name__ == '__main__':
    main()
