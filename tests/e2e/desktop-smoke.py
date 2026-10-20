#!/usr/bin/env python3
"""Exercise the actual WebKitGTK window and Rust IPC through tauri-driver.

Requires a custom-protocol native build (tauri build --debug), tauri-driver,
WebKitWebDriver (webkit2gtk-driver), and a display (xvfb-run in CI).
Uses only the Python standard library. Never opens an audio device.
"""
import base64
import json
import os
from pathlib import Path
import socket
import subprocess
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


PORT = unused_port()
BASE = f'http://127.0.0.1:{PORT}'


def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, method=method,
                                 headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
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


def main():
    session = None
    with (ARTIFACTS / 'desktop-driver.log').open('w') as log:
        command = ['tauri-driver', '--port', str(PORT), '--native-port', str(unused_port())]
        if native_driver := os.environ.get('LYRA_NATIVE_DRIVER'):
            command.extend(['--native-driver', native_driver])
        driver = subprocess.Popen(command, stdout=log, stderr=log)
        try:
            wait_for(lambda: request('GET', '/status'))
            session = request('POST', '/session', {'capabilities': {'alwaysMatch': {
                'tauri:options': {'application': str(ROOT / 'target/debug/lyra-desktop')}
            }}})['sessionId']
            prefix = f'/session/{session}'

            def execute(script):
                return request('POST', prefix + '/execute/sync', {'script': script, 'args': []})

            wait_for(lambda: execute("return document.querySelector('[data-connected=true]') !== null"))
            state = execute("""
                return {
                    title: document.title,
                    albums: document.querySelectorAll('article').length,
                    player: document.querySelector('footer').textContent,
                    status: document.querySelector('[data-testid=backend-status]').textContent,
                    playDisabled: document.querySelector('button[aria-label=Play]').disabled
                };
            """)
            assert state['title'] == 'Lyra — Music player', state
            assert state['albums'] == 3, state
            assert 'Audio engine not implemented' in state['player'], state
            assert state['playDisabled'], state
            assert 'Lyra 0.1.0 · Rust connected · linux' in state['status'], state
            # Retry the connection from the visible UI as well as on initial mount.
            execute("document.querySelector('.backend button').click(); return true")
            wait_for(lambda: execute("return document.querySelector('[data-connected=true]') !== null"))
            png = request('GET', prefix + '/screenshot')
            (ARTIFACTS / 'desktop.png').write_bytes(base64.b64decode(png))
            (ARTIFACTS / 'desktop-smoke.json').write_text(json.dumps(state, indent=2) + '\n')
            print('PASS: native window, 3 mock albums, inactive player, initial and retried Rust IPC')
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
