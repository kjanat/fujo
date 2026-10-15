import { readFileSync, writeFileSync } from 'node:fs';
import { isDeepStrictEqual } from 'node:util';

/** Check semantic JSON equality without rewriting fixtures during validation. */
export function writeReference<T extends { fixtures: readonly unknown[] }>(path: string, value: T): void {
	const serialized = JSON.stringify(value, null, '\t') + '\n';
	if (process.argv.includes('--check')) {
		if (!isDeepStrictEqual(JSON.parse(readFileSync(path, 'utf8')), JSON.parse(serialized))) {
			throw new Error(`Live TypeScript results differ from ${path}; regenerate and review the fixture changes`);
		}
		console.log(`Verified ${value.fixtures.length} live TypeScript cases against ${path}`);
	} else {
		writeFileSync(path, serialized);
		console.log(`Wrote ${value.fixtures.length} reference cases to ${path}`);
	}
}
