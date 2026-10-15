/** Generate behavior fixtures from the unmodified TypeScript checkout. */
import { execFileSync } from 'node:child_process';
import { resolve as resolvePath } from 'node:path';
import { parse } from '../../dreamcli/src/core/parse/index.ts';
import { resolve } from '../../dreamcli/src/core/resolve/index.ts';
import { arg, command, flag } from '../../dreamcli/src/index.ts';
import { writeReference } from './reference-output.ts';

const basic = command('basic')
	.flag('name', flag.string().alias('n').env('NAME').config('app.name').default('default'))
	.flag('port', flag.number().alias('p').env('PORT').config('app.port'))
	.flag('verbose', flag.count().alias('v'))
	.flag('force', flag.boolean().alias('f').negatable().env('FORCE'))
	.flag('region', flag.enum(['us', 'eu']).env('REGION').config('app.region'))
	.flag('tag', flag.array(flag.string()).separator(',').unique().env('TAGS'))
	.flag('define', flag.keyValue().alias('D').env('DEFINES'))
	.flag('timeout', flag.duration().env('TIMEOUT'))
	.flag('size', flag.bytes())
	.flag('date', flag.date())
	.flag('url', flag.url())
	.flag('outputPath', flag.string())
	.arg('target', arg.string().optional());
const required = command('required').flag('token', flag.string().required()).arg('target', arg.string());
const stdin = command('stdin').flag('text', flag.string().stdin().env('TEXT').default('default'));
const list = command('list').flag('tag', flag.array(flag.string()).stdin().env('TAGS'));
const trim = command('trim').arg('path', arg.string().stdin({ trim: true }));
const arraysjson = command('arraysjson').flag(
	'value',
	flag.array(flag.number()).env('VALUE').config('value').stdin().split({ env: 'json', stdin: 'json' }),
);
const entriesjson = command('entriesjson').flag(
	'value',
	flag.keyValue(flag.number()).env('VALUE').config('value').stdin().split({ env: 'json', stdin: 'json' }),
);
const bounds = command('bounds').flag('port', flag.number({ int: true, min: 1, max: 10 }).env('PORT')).flag(
	'name',
	flag.string({ nonEmpty: true, minLength: 2, maxLength: 5, pattern: /^[a-z]+$/ }).env('NAME'),
);
const rest = command('rest').arg('value', arg.number().variadic());
const entriesrest = command('entriesrest').arg('value', arg.keyValue().variadic());
const definitions = { basic, required, stdin, list, trim, arraysjson, entriesjson, bounds, rest, entriesrest };
const cases = [
	['basic', []],
	['basic', ['--name', 'cli'], { env: { NAME: 'env' }, config: { app: { name: 'config' } } }],
	['basic', [], { env: { NAME: 'env' }, config: { app: { name: 'config' } } }],
	['basic', [], { config: { app: { name: 'config', port: 3000, region: 'eu' } } }],
	['basic', ['-vvvf', '-p8080', '-nAda', 'deploy']],
	['basic', ['--force=false', '--no-force', '--force']],
	['basic', ['--name=one', '--name=two']],
	['basic', ['--port=-42']],
	['basic', ['--verbose=2', '-vv']],
	['basic', ['-vv', '--verbose=4', '-v']],
	['basic', ['--tag', 'a,b', '--tag', 'b,c']],
	['basic', [], { env: { TAGS: 'a,b,a' } }],
	['basic', ['-Dx=one', '--define=x=two', '--define=y=a=b']],
	['basic', [], { env: { DEFINES: 'x=one,y=two' } }],
	['basic', ['--output-path=somewhere']],
	['basic', ['--outputPath=somewhere']],
	['basic', ['--timeout=1h30m']],
	['basic', ['--size=1.5MB']],
	['basic', ['--date=2026-10-14']],
	['basic', ['--date=2026-10-14T14:30']],
	['basic', ['--date=2026-10-14T14:30:00+02:00']],
	['basic', ['--url=https://example.com']],
	['basic', ['--', '--json']],
	['basic', [], { env: { FORCE: 'yes', PORT: '42', TIMEOUT: '250ms' } }],
	['stdin', [], { stdinData: 'pipe\n', env: { TEXT: 'env' } }],
	['stdin', ['--text', '-'], { stdinData: 'pipe\n', env: { TEXT: 'env' } }],
	['stdin', ['--text', 'cli'], { stdinData: 'pipe\n' }],
	['stdin', []],
	['list', ['--tag', 'before', '--tag', '-', '--tag', 'after'], { stdinData: 'a\nb\n' }],
	['list', [], { stdinData: 'a\n\nb\n' }],
	['trim', ['-'], { stdinData: './dir\r\n' }],
	['basic', ['--unknown']],
	['basic', ['--name']],
	['basic', ['--force=maybe']],
	['basic', ['--no-force=true']],
	['basic', ['--region=ap']],
	['basic', ['--port=NaN']],
	['basic', ['--port', '-42']],
	['basic', ['--date=2026-02-31']],
	['basic', ['--timeout=1x']],
	['basic', ['--define=invalid']],
	['basic', ['one', 'two']],
	['basic', [], { env: { REGION: 'ap' } }],
	['required', []],
	['required', ['--token=x']],
	['required', ['target']],
	['stdin', ['--text', '-']],
	['basic', ['--tag=a,,b,']],
	['basic', ['--tag=']],
	['basic', [], { env: { TAGS: '' } }],
	['basic', [], { env: { TAGS: ',a,,b,' } }],
	['basic', ['--port=0x10']],
	['basic', ['--port=1e3']],
	['basic', ['--port=']],
	['basic', ['--port=  ']],
	['basic', [], { env: { FORCE: ' yes ' } }],
	['basic', [], { env: { FORCE: '' } }],
	['basic', [], { config: { app: { port: '42', name: 123 } } }],
	['list', ['--tag', '-'], { stdinData: '' }],
	['list', [], { stdinData: '' }],
	['list', ['--tag', 'before', '--tag', '-']],
	['stdin', ['--text', '-'], { stdinData: '' }],
	['stdin', [], { stdinData: '' }],
	['arraysjson', [], { env: { VALUE: '[1,2,3]' } }],
	['arraysjson', [], { stdinData: '[1,2]' }],
	['arraysjson', [], { config: { value: [1, 2] } }],
	['arraysjson', [], { env: { VALUE: 'invalid' } }],
	['arraysjson', [], { env: { VALUE: '{}' } }],
	['arraysjson', [], { env: { VALUE: '[1,"bad"]' } }],
	['entriesjson', [], { env: { VALUE: '{"x":1,"y":2}' } }],
	['entriesjson', [], { stdinData: '{"x":1}' }],
	['entriesjson', [], { config: { value: { x: 1, y: 2 } } }],
	['entriesjson', [], { env: { VALUE: '[]' } }],
	['bounds', ['--port=1', '--name=abc']],
	['bounds', ['--port=0']],
	['bounds', ['--port=1.5']],
	['bounds', ['--port=11']],
	['bounds', ['--name=']],
	['bounds', ['--name=Ada']],
	['bounds', [], { env: { PORT: '0' } }],
	['bounds', [], { env: { NAME: 'ABC' } }],
	['rest', ['1', '2', '3']],
	['rest', []],
	['rest', ['not-a-number']],
	['entriesjson', [], { config: { value: [] } }],
	['entriesjson', [], { config: { value: ['x=1'] } }],
	['entriesrest', ['x=one', 'y=two']],
	['entriesrest', ['x=one', 'x=two']],
	['entriesrest', []],
] as const;
const fixtures = [];
for (const [schema, argv, options = {}] of cases) {
	try {
		const parsed = parse(definitions[schema].schema, argv);
		const result = await resolve(definitions[schema].schema, parsed, options);
		fixtures.push({
			schema,
			argv,
			options,
			expected: { flags: result.flags, args: result.args, sources: result.provenance },
		});
	} catch (error) {
		const e = error as { code: string; exitCode: number };
		fixtures.push({ schema, argv, options, error: { code: e.code, exitCode: e.exitCode } });
	}
}
const path = resolvePath(import.meta.dir, '../crates/dreamcli/tests/fixtures/reference.json');
writeReference(
	path,
	{
		sourceCommit: execFileSync('git', ['-C', resolvePath(import.meta.dir, '../../dreamcli'), 'rev-parse', 'HEAD'], {
			encoding: 'utf8',
		}).trim(),
		fixtures,
	},
);
