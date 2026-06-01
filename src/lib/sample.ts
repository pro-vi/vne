import type { ProjectSnapshot } from './types';

const basePath = '/demo/project/.env';
const examplePath = '/demo/project/.env.example';
const localPath = '/demo/project/.env.local';
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
          entry('DATABASE_URL', 'postgres://localhost/vne', 'Credential URL / DSN', 'credential-url', true, 2, [
            'credential-bearing URL key name',
            'URL-like name or value'
          ]),
          entry('REDIS_URL', 'redis://localhost:6379', 'Credential URL / DSN', 'credential-url', true, 3, [
            'credential-bearing URL key name',
            'URL-like name or value'
          ]),
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
          entry('DATABASE_URL', '', 'Credential URL / DSN', 'credential-url', true, 1, [
            'credential-bearing URL key name',
            'URL-like name or value'
          ]),
          entry('REDIS_URL', '', 'Credential URL / DSN', 'credential-url', true, 2, [
            'credential-bearing URL key name',
            'URL-like name or value'
          ]),
          entry('STRIPE_SECRET_KEY', '', 'Secret', 'secret', true, 3, ['Stripe convention', 'secret-like key name']),
          entry('NEXT_PUBLIC_SITE_URL', '', 'Public frontend variable', 'public', false, 4, ['frontend-exposed prefix']),
          entry('PORT', '1420', 'Port', 'port', false, 5, ['port-like key or numeric port value'])
        ]
      },
      {
        path: localPath,
        name: '.env.local',
        discoveryReasons: ['direct env filename `.env.local`'],
        diagnostics: [],
        duplicateKeys: [],
        content: 'DATABASE_URL="postgres://localhost/vne_local"\nOPENAI_API_KEY=sk-local-override\n',
        entries: [
          entry('DATABASE_URL', 'postgres://localhost/vne_local', 'Credential URL / DSN', 'credential-url', true, 1, [
            'credential-bearing URL key name',
            'URL-like name or value'
          ]),
          entry('OPENAI_API_KEY', 'sk-local-override', 'Secret', 'secret', true, 2, [
            'OpenAI convention',
            'secret-like key name'
          ])
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
    layerReport: {
      orderedFiles: [
        {
          path: basePath,
          name: '.env',
          layerKind: 'base',
          precedence: 10
        },
        {
          path: '/demo/project/.env.local',
          name: '.env.local',
          layerKind: 'local',
          precedence: 30
        },
        {
          path: workerPath,
          name: 'worker.env',
          layerKind: 'referenced',
          precedence: 60
        }
      ],
      overrides: [
        {
          key: 'DATABASE_URL',
          files: ['.env', '.env.local'],
          effectiveFile: '.env.local',
          conflict: true,
          redacted: true,
          summary: '2 layers set different values; `.env.local` currently wins.'
        },
        {
          key: 'OPENAI_API_KEY',
          files: ['.env', '.env.local'],
          effectiveFile: '.env.local',
          conflict: true,
          redacted: true,
          summary: '2 layers set different values; `.env.local` currently wins.'
        }
      ],
      placeholderKeys: []
    },
    frameworkProfiles: [
      {
        framework: 'Next.js',
        mode: 'development',
        evidence: ['package.json dependencies.next = ^14.2.0'],
        orderedFiles: [
          {
            path: localPath,
            name: '.env.local',
            layerKind: 'local',
            rank: 2
          },
          {
            path: basePath,
            name: '.env',
            layerKind: 'base',
            rank: 4
          }
        ],
        missingFiles: ['.env.development.local', '.env.development'],
        notes: [
          'Effective order is highest priority first; process.env is checked before files.',
          'Next.js stops lookup once a key is found.',
          '`NEXT_PUBLIC_` keys are browser-exposed by convention.'
        ]
      }
    ],
    repairActions: [
      {
        severity: 'warning',
        actionKind: 'add-missing-key',
        title: 'Add `STRIPE_SECRET_KEY` to .env',
        detail: '`STRIPE_SECRET_KEY` is documented in .env.example but missing from .env.',
        filePath: basePath,
        key: 'STRIPE_SECRET_KEY'
      },
      {
        severity: 'warning',
        actionKind: 'resolve-duplicate-key',
        title: 'Resolve duplicate `FEATURE_ENABLED`',
        detail: '`.env` defines `FEATURE_ENABLED` more than once.',
        filePath: null,
        key: 'FEATURE_ENABLED'
      },
      {
        severity: 'info',
        actionKind: 'review-layer-conflict',
        title: 'Review layered `DATABASE_URL`',
        detail: '.env, .env.local set `DATABASE_URL` in multiple env layers; .env.local currently wins.',
        filePath: null,
        key: 'DATABASE_URL'
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
    id: `${key}@${lineNumber}`,
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
