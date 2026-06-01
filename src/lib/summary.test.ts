import { describe, expect, it } from 'vitest';
import { sampleProject } from './sample';
import { isEntryValueHidden, keyStatus, totalIssueCount } from './summary';

describe('env summary helpers', () => {
  it('counts comparison and diagnostic issues', () => {
    expect(totalIssueCount(sampleProject())).toBe(9);
  });

  it('marks extra and missing comparison keys in the relevant files', () => {
    const snapshot = sampleProject();
    const actual = snapshot.files[0];
    const example = snapshot.files[1];

    expect(keyStatus(actual, actual.entries.find((entry) => entry.key === 'OPENAI_API_KEY')!, snapshot.comparison)).toBe('extra');
    expect(keyStatus(example, example.entries.find((entry) => entry.key === 'STRIPE_SECRET_KEY')!, snapshot.comparison)).toBe(
      'missing'
    );
  });

  it('hides secret values unless that entry is revealed', () => {
    const snapshot = sampleProject();
    const secret = snapshot.files[0].entries.find((entry) => entry.key === 'OPENAI_API_KEY')!;
    const publicEntry = snapshot.files[0].entries.find((entry) => entry.key === 'NEXT_PUBLIC_SITE_URL')!;
    const publicSecret = snapshot.files[0].entries.find((entry) => entry.key === 'NEXT_PUBLIC_API_KEY')!;

    expect(isEntryValueHidden(secret, false)).toBe(true);
    expect(isEntryValueHidden(secret, true)).toBe(false);
    expect(isEntryValueHidden(publicEntry, false)).toBe(false);
    expect(publicEntry.shape.exposure).toBe('browser');
    expect(publicEntry.shape.sensitive).toBe(false);
    expect(isEntryValueHidden(publicSecret, false)).toBe(true);
    expect(publicSecret.shape.exposure).toBe('browser');
    expect(publicSecret.shape.sensitive).toBe(true);
  });
});
