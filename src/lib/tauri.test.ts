import { describe, expect, it } from 'vitest';
import { relativePath } from './paths';
import { parseClipboardCopyOutcome, parseConcealedCopyExpired, parseEnsureEnvFileOutcome } from './tauri';

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

describe('native clipboard boundary', () => {
  it('accepts a concealed copy with both fields and a plain copy with neither', () => {
    expect(parseClipboardCopyOutcome({ copyId: 148, clearsInSeconds: 30 })).toEqual({ copyId: 148, clearsInSeconds: 30 });
    expect(parseClipboardCopyOutcome({ copyId: null, clearsInSeconds: null })).toEqual({ copyId: null, clearsInSeconds: null });
  });

  it.each([
    null,
    { clearsInSeconds: 30 },
    { copyId: 148, clearsInSeconds: null },
    { copyId: null, clearsInSeconds: 30 },
    { copyId: 1.5, clearsInSeconds: 30 },
    { copyId: 148, clearsInSeconds: 0 }
  ])('rejects a copy outcome that would misstate the clear: %j', (value) => {
    expect(() => parseClipboardCopyOutcome(value)).toThrow('native copy response was malformed');
  });

  it('accepts only a well-formed expiry event', () => {
    expect(parseConcealedCopyExpired({ copyId: 148, cleared: false })).toEqual({ copyId: 148, cleared: false });
    expect(() => parseConcealedCopyExpired({ copyId: 148 })).toThrow('native clipboard event was malformed');
    expect(() => parseConcealedCopyExpired({ copyId: '148', cleared: true })).toThrow('native clipboard event was malformed');
  });
});

describe('paths sent to the backend', () => {
  it('makes a project path relative and leaves an outside path for the backend to refuse', () => {
    expect(relativePath('/demo/project', '/demo/project/config/worker.env')).toBe('config/worker.env');
    expect(relativePath('/demo/project/', '/demo/project/.env')).toBe('.env');
    expect(relativePath('/demo/project', '/elsewhere/.env')).toBe('/elsewhere/.env');
  });
});
