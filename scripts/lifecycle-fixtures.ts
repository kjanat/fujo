/** Generate orchestration expectations from the recorded TypeScript revision. */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { PROMPT_CANCEL } from '../../dreamcli/src/core/prompt/test-prompter.ts';
import { runCommand } from '../../dreamcli/src/core/testkit/index.ts';
import { CLIError, command, flag, middleware } from '../../dreamcli/src/index.ts';
import { writeReference } from './reference-output.ts';

const sourceCommit = execFileSync('git', ['-C', resolve(import.meta.dir, '../../dreamcli'), 'rev-parse', 'HEAD'], {
	encoding: 'utf8',
}).trim();
const baseline = JSON.parse(
	readFileSync(resolve(import.meta.dir, '../crates/dreamcli/tests/fixtures/reference.json'), 'utf8'),
);
if (sourceCommit !== baseline.sourceCommit) {
	throw new Error('TypeScript checkout must match reference.json sourceCommit');
}
const fixtures = [];
for (
	const name of [
		'success',
		'short-circuit',
		'derive-failure',
		'action-failure',
		'transform-error',
		'exit-code',
		'activities',
		'activity-failure',
		'json',
	]
) {
	const trace: string[] = [];
	const outer = middleware(async ({ next }) => {
		trace.push('outer:before');
		try {
			await next({});
		} catch (error) {
			trace.push('caught');
			if (name === 'transform-error') throw new CLIError('wrapped', { code: 'WRAPPED', exitCode: 2 });
			throw error;
		} finally {
			trace.push('outer:after');
		}
	});
	const inner = middleware(async ({ next }) => {
		trace.push('inner:before');
		try {
			if (name !== 'short-circuit') await next({});
		} finally {
			trace.push('inner:after');
		}
	});
	const cmd = command('demo').middleware(outer).middleware(inner)
		.derive(async () => {
			trace.push('derive');
			await Promise.resolve();
			if (name === 'derive-failure') throw new CLIError('denied', { code: 'DENIED', exitCode: 2 });
			return { value: 7 };
		})
		.action(async ({ out, ctx }) => {
			trace.push('action');
			await Promise.resolve();
			if (name === 'activity-failure') out.spinner('work');
			if (['action-failure', 'transform-error', 'activity-failure'].includes(name)) {
				throw new CLIError('failed', { code: 'FAILED', exitCode: 2 });
			}
			if (name === 'activities') {
				out.spinner('work').succeed('ready');
				const progress = out.progress('items', 2);
				progress.update(1);
				progress.done('done');
			}
			if (name === 'exit-code') out.setExitCode(3);
			if (name === 'json') out.json({ value: ctx.value });
			else out.log(String(ctx.value));
		});
	const result = await runCommand(cmd, [], { jsonMode: name === 'json' });
	fixtures.push({
		name,
		trace,
		exitCode: result.exitCode,
		code: result.error?.code ?? null,
		stdout: result.stdout.join(''),
		starts: result.activity.filter(e => e.type.endsWith(':start')).length,
		finishes: result.activity.filter(e => /:(succeed|fail|done|stop)$/.test(e.type)).length,
	});
}
for (const required of [false, true]) {
	const input = flag.string().prompt({ kind: 'input', message: 'Name?' });
	const cmd = command('demo').flag('name', required ? input.required() : input.default('fallback'))
		.action(({ flags, out }) => out.log(flags.name));
	const result = await runCommand(cmd, [], { answers: [PROMPT_CANCEL] });
	fixtures.push({
		name: required ? 'prompt-skip-required' : 'prompt-skip-default',
		trace: [],
		exitCode: result.exitCode,
		code: result.error?.code ?? null,
		stdout: result.stdout.join(''),
		starts: 0,
		finishes: 0,
	});
}
const path = resolve(import.meta.dir, '../crates/dreamcli/tests/fixtures/lifecycle.json');
writeReference(path, { sourceCommit, fixtures });
