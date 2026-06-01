import type { ProjectSnapshot } from './types';

const basePath = '/demo/project/.env';
const examplePath = '/demo/project/.env.example';
const workerPath = '/demo/project/config/worker.env';

export function sampleProject(root = '/demo/project'): ProjectSnapshot {
  return {
    root,
    files: [
      {
        path: basePath,
        name: '.env',
        discoveryReasons: ['direct env filename `.env`'],
        diagnostics: ['Duplicate key `FEATURE_ENABLED`'],
        duplicateKeys: ['FEATURE_ENABLED'],
        content:
          '# local development\nDATABASE_URL="postgres://localhost/vne"\nREDIS_URL=redis://localhost:6379\nOPENAI_API_KEY=sk-local-redacted\nPRIVATE_KEY="-----BEGIN KEY-----\nabc123\n-----END KEY-----"\nNEXT_PUBLIC_SITE_URL=http://localhost:1420\nPORT=1420\nFEATURE_ENABLED=true\nFEATURE_ENABLED=false\n',
        entries: [
          entry('DATABASE_URL', 'postgres://localhost/vne', 'URL / DSN', 'url', false, 2, ['URL-like name or value']),
          entry('REDIS_URL', 'redis://localhost:6379', 'URL / DSN', 'url', false, 3, ['URL-like name or value']),
          entry('OPENAI_API_KEY', 'sk-local-redacted', 'Secret', 'secret', true, 4, ['OpenAI convention', 'secret-like key name']),
          entry('PRIVATE_KEY', '-----BEGIN KEY-----\nabc123\n-----END KEY-----', 'Secret', 'secret', true, 5, [
            'secret-like key name'
          ]),
          entry('NEXT_PUBLIC_SITE_URL', 'http://localhost:1420', 'Public frontend variable', 'public', false, 8, [
            'frontend-exposed prefix'
          ]),
          entry('PORT', '1420', 'Port', 'port', false, 9, ['port-like key or numeric port value']),
          entry('FEATURE_ENABLED', 'true', 'Boolean', 'bool', false, 10, ['boolean-like value'], ['Duplicate key']),
          entry('FEATURE_ENABLED', 'false', 'Boolean', 'bool', false, 11, ['boolean-like value'], ['Duplicate key'])
        ]
      },
      {
        path: examplePath,
        name: '.env.example',
        discoveryReasons: ['direct env filename `.env.example`'],
        diagnostics: [],
        duplicateKeys: [],
        content: 'DATABASE_URL=\nREDIS_URL=\nSTRIPE_SECRET_KEY=\nNEXT_PUBLIC_SITE_URL=\nPORT=1420\n',
        entries: [
          entry('DATABASE_URL', '', 'URL / DSN', 'url', false, 1, ['URL-like name or value']),
          entry('REDIS_URL', '', 'URL / DSN', 'url', false, 2, ['URL-like name or value']),
          entry('STRIPE_SECRET_KEY', '', 'Secret', 'secret', true, 3, ['Stripe convention', 'secret-like key name']),
          entry('NEXT_PUBLIC_SITE_URL', '', 'Public frontend variable', 'public', false, 4, ['frontend-exposed prefix']),
          entry('PORT', '1420', 'Port', 'port', false, 5, ['port-like key or numeric port value'])
        ]
      },
      {
        path: workerPath,
        name: 'worker.env',
        discoveryReasons: ['package.json script `worker`'],
        diagnostics: [],
        duplicateKeys: [],
        content: 'QUEUE_URL=redis://localhost:6379\nWORKER_CONCURRENCY=4\n',
        entries: [
          entry('QUEUE_URL', 'redis://localhost:6379', 'URL / DSN', 'url', false, 1, ['URL-like name or value']),
          entry('WORKER_CONCURRENCY', '4', 'Integer', 'int', false, 2, ['integer value'])
        ]
      }
    ],
    comparison: {
      basePath,
      examplePath,
      missingKeys: ['STRIPE_SECRET_KEY'],
      extraKeys: ['OPENAI_API_KEY', 'PRIVATE_KEY', 'FEATURE_ENABLED'],
      sharedKeys: ['DATABASE_URL', 'NEXT_PUBLIC_SITE_URL', 'PORT', 'REDIS_URL'],
      duplicateKeys: ['.env:FEATURE_ENABLED']
    }
  };
}

function entry(
  key: string,
  value: string,
  label: string,
  kind: string,
  redacted: boolean,
  lineNumber: number,
  reasons: string[],
  diagnostics: string[] = []
) {
  return {
    key,
    value,
    displayValue: redacted ? '********' : value,
    lineNumber,
    exported: false,
    quote: null,
    diagnostics,
    shape: {
      kind,
      label,
      confidence: kind === 'text' ? 'low' : 'high',
      redactedByDefault: redacted,
      reasons
    }
  };
}
