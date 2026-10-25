// Node.js 22.18+ or 24+: node --test scripts/test-token-stats-date-range.mjs
import assert from 'node:assert/strict';
import test from 'node:test';
import { getStatsRangeBounds, restoreStatsDateRange } from '../src/utils/tokenStatsDateRange.ts';

const now = new Date(2026, 9, 26, 15, 23, 12, 456);
const seconds = Math.floor(now.getTime() / 1000);

test('today starts at local midnight and includes the current second', () => {
    assert.deepEqual(getStatsRangeBounds({ preset: 'today' }, now), {
        startTime: new Date(2026, 9, 26).getTime() / 1000,
        endTime: seconds + 1,
        bucket: 'hourly',
    });
});

test('rolling presets use exact elapsed days and choose the right bucket', () => {
    for (const [preset, days] of [['24h', 1], ['7d', 7], ['14d', 14], ['30d', 30]]) {
        assert.deepEqual(getStatsRangeBounds({ preset }, now), {
            startTime: seconds - days * 86400, endTime: seconds + 1,
            bucket: days === 1 ? 'hourly' : 'daily',
        });
    }
});

test('all history starts at the epoch and uses daily buckets', () => {
    assert.deepEqual(getStatsRangeBounds({ preset: 'all' }, now), {
        startTime: 0, endTime: seconds + 1, bucket: 'daily',
    });
});

test('custom dates keep exact start-inclusive and end-exclusive boundaries', () => {
    const range = { preset: 'custom', start: '2026-10-20T08:30', end: '2026-10-20T09:45' };
    assert.deepEqual(getStatsRangeBounds(range, now), {
        startTime: new Date(range.start).getTime() / 1000,
        endTime: new Date(range.end).getTime() / 1000, bucket: 'hourly',
    });
    assert.equal(getStatsRangeBounds({ ...range, end: '2026-10-22T09:45' }, now).bucket, 'daily');
});

test('invalid custom dates and preferences fall back without breaking the page', () => {
    for (const range of [
        { preset: 'custom', start: '', end: '' },
        { preset: 'custom', start: 1, end: 999999 },
        { preset: 'custom', start: '2026-10-20T08:30', end: '2026-10-20T08:30' },
        { preset: 'custom', start: '2026-10-21T08:30', end: '2026-10-20T08:30' },
        { preset: 'custom', start: 'bad', end: '2026-10-20T08:30' },
    ]) {
        assert.throws(() => getStatsRangeBounds(range, now));
        assert.deepEqual(restoreStatsDateRange(JSON.stringify(range)), { preset: '7d' });
    }
    for (const saved of [null, 'not json', '"daily"', '{"preset":"weekly"}']) {
        assert.deepEqual(restoreStatsDateRange(saved), { preset: '7d' });
    }
    assert.deepEqual(restoreStatsDateRange('{"preset":"all"}'), { preset: 'all' });
});
