// The workday rules. Run with bun to write storyline.json.
import { fileURLToPath } from 'node:url';
import { check, writeResult } from '../../../tinycharts/tinycharts/build/check-lib.mjs';

const HERE = fileURLToPath(new URL('.', import.meta.url));

export const result = check({
  mmd: HERE + 'storyline.mmd',
  init: { due: false, skips: 0, rested: false },
  guard: {
    'break due': v => v.due,
    'under the limit': v => v.skips < 2,
    'skip limit reached': v => v.skips >= 2,
    'eyes closed': v => v.rested,
  },
  effect: {
    'break due': v => ({ ...v, due: true }),
    skip: v => ({ ...v, skips: v.skips + 1 }),
    'close eyes': v => ({ ...v, rested: true, skips: 0 }),
    'another day': () => ({ due: false, skips: 0, rested: false }),
  },
  rules: [
    { rule: 'Rest before you ship', says: 'The day ships only after the eyes really close for the break.', holds: s => s.at !== 'shipped' || s.rested },
    { rule: 'Two skips at most', says: 'After two skips in a row, Skip and Postpone lock.', holds: s => s.skips <= 2 },
  ],
  mutants: { 'eyes closed': true, 'under the limit': true },
});

if (process.argv[1] === fileURLToPath(import.meta.url)) writeResult(HERE + 'storyline.json', result);
