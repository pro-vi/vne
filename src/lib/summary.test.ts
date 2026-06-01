import { describe, expect, it } from 'vitest';
import { sampleProject } from './sample';
import { keyStatus, totalIssueCount } from './summary';

describe('env summary helpers', () => {
  it('counts comparison and diagnostic issues', () => {
    expect(totalIssueCount(sampleProject())).toBe(5);
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
});
