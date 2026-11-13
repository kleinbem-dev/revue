import assert from 'node:assert/strict';
import test from 'node:test';
import { buildApiHeaders } from '../src/lib/api.ts';
import {
	computeNoiseReductionPct,
	generateRevueYaml,
	mapApiFinding,
	normalizeSeverity,
	severityBucket
} from '../src/lib/review-utils.ts';

test('normalizeSeverity and severityBucket classify all backend severities', () => {
	assert.equal(normalizeSeverity('SECURITY'), 'security');
	assert.equal(severityBucket('security'), 'critical');
	assert.equal(severityBucket('critical'), 'critical');

	assert.equal(severityBucket('bug'), 'major');
	assert.equal(severityBucket('error'), 'major');

	assert.equal(severityBucket('performance'), 'minor');
	assert.equal(severityBucket('warning'), 'minor');
	assert.equal(severityBucket('info'), 'minor');

	assert.equal(normalizeSeverity('unexpected-value'), 'warning');
	assert.equal(normalizeSeverity(undefined), 'warning');
});

test('mapApiFinding preserves feedback, aliases, and fallbacks', () => {
	const mapped = mapApiFinding({
		file: 'src/db.rs',
		line: 42,
		severity: 'security',
		title: 'SQL injection',
		comment: 'Use parameterized query',
		suggestion: 'sqlx::query(...).bind(id)',
		confidence: 9,
		feedback: 'false_positive'
	});

	assert.equal(mapped.file, 'src/db.rs');
	assert.equal(mapped.line, 42);
	assert.equal(mapped.severity, 'security');
	assert.equal(mapped.body, 'Use parameterized query');
	assert.equal(mapped.snippet, 'sqlx::query(...).bind(id)');
	assert.equal(mapped.feedback, 'false_positive');
});

test('computeNoiseReductionPct returns null on zero candidates instead of fake metrics', () => {
	assert.equal(computeNoiseReductionPct(0, 0), null);
	assert.equal(computeNoiseReductionPct(-5, 2), null);
	assert.equal(computeNoiseReductionPct(10, 3), 70);
	assert.equal(computeNoiseReductionPct(10, 0), 100);
	assert.equal(computeNoiseReductionPct(4, 10), 0);
});

test('generateRevueYaml emits plain globs without negated ! prefixes', () => {
	const yaml = generateRevueYaml({
		min_severity: 'warning',
		summary: false,
		two_pass: true,
		semgrep: true,
		ignore_lockfiles: true,
		ignore_vendor: true,
		ignore_generated: false,
		tone: 'Direct and pragmatic.',
		custom_guidelines: 'Never use unwrap() in handlers.\nPrefer structured errors.'
	});

	assert.match(yaml, /version: 1/);
	assert.match(yaml, /summary: false/);
	assert.match(yaml, /"(\*\*\/Cargo\.lock)"/);
	assert.doesNotMatch(yaml, /!\*\*\//);
	assert.match(yaml, /Never use unwrap\(\) in handlers\./);
});

test('buildApiHeaders attaches X-Revue-Key only when non-empty and absent', () => {
	const withKey = buildApiHeaders({ 'Content-Type': 'application/json' }, '  secret-123  ');
	assert.equal(withKey.get('X-Revue-Key'), 'secret-123');
	assert.equal(withKey.get('Content-Type'), 'application/json');

	const withoutKey = buildApiHeaders(undefined, '   ');
	assert.equal(withoutKey.get('X-Revue-Key'), null);
});
