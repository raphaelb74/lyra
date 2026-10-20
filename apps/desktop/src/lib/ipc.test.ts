import { describe, expect, it, vi } from 'vitest';

import { getAppInfo } from './ipc';

const info = { name: 'Lyra', version: '0.1.0', platform: 'linux' };

describe('desktop IPC boundary', () => {
  it('requests the registered command and accepts the Rust response shape', async () => {
    const runCommand = vi.fn(() => Promise.resolve(info));

    await expect(getAppInfo(runCommand, true)).resolves.toEqual(info);
    expect(runCommand).toHaveBeenCalledExactlyOnceWith('get_app_info');
  });

  it('identifies browser preview without attempting IPC', async () => {
    const runCommand = vi.fn(() => Promise.resolve(info));

    await expect(getAppInfo(runCommand, false)).rejects.toMatchObject({
      code: 'browser_preview',
    });
    expect(runCommand).not.toHaveBeenCalled();
  });

  it.each([null, 'Lyra', {}, { name: 'Lyra', version: 1, platform: 'linux' }])(
    'rejects malformed backend data: %j',
    async (value) => {
      await expect(
        getAppInfo(() => Promise.resolve(value), true),
      ).rejects.toMatchObject({ code: 'invalid_response' });
    },
  );

  it.each([
    ['a string', 'get_app_info not allowed', 'get_app_info not allowed'],
    ['an error object', { message: 'Backend crashed' }, 'Backend crashed'],
    ['anything else', null, 'Try checking the backend again'],
  ])(
    'turns a rejection with %s into a readable error',
    async (_, cause, message) => {
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- Tauri rejects with plain strings and objects; reproduce that.
      const result = getAppInfo(() => Promise.reject(cause), true);

      await expect(result).rejects.toThrow(message);
      await expect(result).rejects.toMatchObject({ code: 'command_failed' });
    },
  );
});
