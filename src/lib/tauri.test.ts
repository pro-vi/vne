import { describe, expect, it } from 'vitest';
import { parseEnsureEnvFileOutcome } from './tauri';

describe('native env-file creation boundary', () => {
  it('accepts the exact creation dispositions', () => {
    expect(parseEnsureEnvFileOutcome({ disposition: 'created', path: '/project/.env' })).toEqual({
      disposition: 'created',
      path: '/project/.env'
    });
    expect(parseEnsureEnvFileOutcome({ disposition: 'alreadyExists', path: '/project/.env' })).toEqual({
      disposition: 'alreadyExists',
      path: '/project/.env'
    });
  });

  it.each([
    null,
    {},
    { disposition: 'created' },
    { disposition: 'overwritten', path: '/project/.env' },
    { disposition: 'created', path: '' },
    { disposition: 'created', path: 42 }
  ])('rejects malformed native output: %j', (value) => {
    expect(() => parseEnsureEnvFileOutcome(value)).toThrow('native create-file response was malformed');
  });
});
