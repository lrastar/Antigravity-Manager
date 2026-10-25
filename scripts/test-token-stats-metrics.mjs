// Run with Node.js 22.18+ or 24+: node --test scripts/test-token-stats-metrics.mjs
import assert from 'node:assert/strict';
import test from 'node:test';
import { estimateTokenCost, getCacheHitRate, getTokenUsageMetrics, resolveModelPricing } from '../src/utils/tokenStatsMetrics.ts';

const pricing = { input: 2, output: 10, cached: 0.2 };
const proxy = {
    total_input_tokens: 1_000_000,
    total_cached_tokens: 800_000,
    total_output_tokens: 100_000,
    total_tokens: 1_100_000,
    request_count: 2,
    native_generation_count: 0,
};
const native = {
    total_input_tokens: 2_000_000,
    total_cached_tokens: 1_800_000,
    total_output_tokens: 200_000,
    total_tokens: 2_200_000,
    request_count: 0,
    native_generation_count: 3,
    proxy_input_tokens: 0,
    proxy_output_tokens: 0,
    proxy_cached_tokens: 0,
    proxy_total_tokens: 0,
};
const combined = {
    total_input_tokens: 3_000_000,
    total_cached_tokens: 2_600_000,
    total_output_tokens: 300_000,
    total_tokens: 3_300_000,
    request_count: 2,
    native_generation_count: 3,
    proxy_input_tokens: proxy.total_input_tokens,
    proxy_output_tokens: proxy.total_output_tokens,
    proxy_cached_tokens: proxy.total_cached_tokens,
    proxy_total_tokens: proxy.total_tokens,
};

function close(actual, expected) {
    assert.ok(Math.abs(actual - expected) < 1e-10, `${actual} != ${expected}`);
}

test('native estimates include cache reads at their own rate without double charging input', () => {
    close(estimateTokenCost(getTokenUsageMetrics(native, 'antigravity'), pricing), 2.76);
    close(getCacheHitRate(native.total_input_tokens, native.total_cached_tokens), 90);
});

test('all-source cost includes native usage and equals the two source estimates', () => {
    const all = getTokenUsageMetrics(combined, 'all');
    const proxyCost = estimateTokenCost(getTokenUsageMetrics(proxy, 'proxy'), pricing);
    const nativeCost = estimateTokenCost(getTokenUsageMetrics(native, 'antigravity'), pricing);
    close(estimateTokenCost(all, pricing), 4.32);
    close(estimateTokenCost(all, pricing), proxyCost + nativeCost);
    assert.equal(all.requestCount, 5);
    close(getCacheHitRate(all.inputTokens, all.cachedTokens), 86.66666666666667);
});

test('summary counts and grouped counts use the same denominator for every source', () => {
    for (const [source, record] of [['proxy', proxy], ['antigravity', native], ['all', combined]]) {
        const summary = {
            total_input_tokens: record.total_input_tokens,
            total_output_tokens: record.total_output_tokens,
            total_cached_tokens: record.total_cached_tokens,
            total_tokens: record.total_tokens,
            total_requests: record.request_count,
            total_native_generations: record.native_generation_count,
        };
        assert.deepEqual(getTokenUsageMetrics(summary, source), getTokenUsageMetrics(record, source));
    }
});

test('empty data has no estimated charges, calls or cache hits', () => {
    for (const source of ['all', 'proxy', 'antigravity']) {
        const metrics = getTokenUsageMetrics({}, source);
        assert.equal(metrics.requestCount, 0);
        assert.equal(estimateTokenCost(metrics, pricing), 0);
        assert.equal(getCacheHitRate(metrics.inputTokens, metrics.cachedTokens), 0);
    }
});

test('cached input is not added again to the denominator and hits stay within 100 percent', () => {
    assert.equal(getCacheHitRate(1_000, 967), 96.7);
    assert.equal(getCacheHitRate(1_000, 1_000), 100);
    assert.equal(getCacheHitRate(0, 0), 0);
    assert.equal(getCacheHitRate(100, 110), 100);
});

test('cache writes already normalized into non-read input use the input price', () => {
    const metrics = { inputTokens: 1_000, outputTokens: 100, cachedTokens: 600 };
    // 400 non-read input tokens (including writes), 600 reads, 100 output tokens.
    close(estimateTokenCost(metrics, pricing), 0.00192);
});

test('dated and provider-prefixed models use their specific price before a broader family', () => {
    const family = { input: 2.5, output: 10, cached: 1.25 };
    const mini = { input: 0.15, output: 0.6, cached: 0.075 };
    const rules = { 'gpt-4o': family, 'gpt-4o-mini': mini, default: pricing };
    assert.deepEqual(resolveModelPricing('OpenAI/gpt-4o-mini-2024-07-18', rules, pricing), mini);
    assert.deepEqual(resolveModelPricing('gpt-4o-high', rules, pricing), family);
});

test('custom prices support dotted names and exact overrides without unrelated substring matches', () => {
    const custom = { input: 3, output: 15, cached: 0.3 };
    const tier = { input: 4, output: 20, cached: 0.4 };
    const rules = { 'claude-sonnet-4-6': custom, 'claude-sonnet-4-6-high': tier, o1: custom, default: pricing };
    assert.deepEqual(resolveModelPricing('Claude-Sonnet-4.6', rules, pricing), custom);
    assert.deepEqual(resolveModelPricing('claude-sonnet-4.6-high', rules, pricing), tier);
    assert.deepEqual(resolveModelPricing('foo1', rules, pricing), pricing);
    assert.deepEqual(resolveModelPricing('unknown', {}, pricing), pricing);
});
