#!/usr/bin/env python3
"""Verify `tauri dev` builds and opens its native window; stop all test processes."""
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
ARTIFACTS = ROOT / 'artifacts'


def main():
    ARTIFACTS.mkdir(exist_ok=True)
    with (ARTIFACTS / 'tauri-dev.log').open('w') as log:
        process = subprocess.Popen(
            ['npm', 'run', 'tauri', 'dev', '--', '--no-watch'],
            cwd=ROOT / 'apps/desktop', stdout=log, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            deadline = time.monotonic() + 600
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError(f'tauri dev exited {process.returncode}; see artifacts/tauri-dev.log')
                result = subprocess.run(
                    ['xdotool', 'search', '--onlyvisible', '--name', '^Lyra'],
                    capture_output=True, text=True, check=False,
                )
                if result.returncode == 0 and result.stdout.strip():
                    print('PASS: npm run tauri dev built and opened the Lyra native window')
                    return
                time.sleep(0.5)
            raise RuntimeError('Timed out waiting for the Lyra dev window; see artifacts/tauri-dev.log')
        finally:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()


if __name__ == '__main__':
    main()
