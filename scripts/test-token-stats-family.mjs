import assert from 'node:assert/strict';
import test from 'node:test';
import { groupModelTokenStatsByFamily, aggregateModelTrendsByFamily } from '../src/utils/modelFamily.ts';
import { getTokenUsageMetrics, estimateTokenCost, resolveModelPricing } from '../src/utils/tokenStatsMetrics.ts';

const pricing = { input: 2, output: 10, cached: 0.2 };
const variant = (model, output, requestCount, nativeCount, throughput) => ({
    model, total_input_tokens: 1000, total_cached_tokens: 800,
    total_output_tokens: output, total_tokens: 1000 + output,
    request_count: requestCount, native_generation_count: nativeCount, throughput,
});

test('family groups preserve native calls, variant prices, and duration-weighted speed', () => {
    const stats = [
        variant('gemini-3.8-flash-high', 100, 1, 0, { tokens_per_second: 100, timed_generations: 1, timed_output_tokens: 100, total_duration_ms: 1000 }),
        variant('gemini-3.8-flash-low', 900, 0, 2, { tokens_per_second: 300, timed_generations: 2, timed_output_tokens: 900, total_duration_ms: 3000 }),
        variant('gemini-3.8-flash', 99999, 0, 1, { tokens_per_second: null, timed_generations: 0, timed_output_tokens: 0, total_duration_ms: 0 }),
    ];
    const cost = model => estimateTokenCost(getTokenUsageMetrics(model, 'all'), pricing);
    const [family] = groupModelTokenStatsByFamily(stats, cost);
    assert.equal(family.familyId, 'gemini-3.8-flash');
    assert.equal(getTokenUsageMetrics(family, 'all').requestCount, 4);
    assert.equal(family.total_tokens, stats.reduce((sum, model) => sum + model.total_tokens, 0));
    assert.equal(family.cost, stats.reduce((sum, model) => sum + cost(model), 0));
    assert.equal(family.throughput.tokens_per_second, 250);
    assert.equal(family.throughput.timed_generations, 3);
    assert.equal(family.variants.find(model => model.model.endsWith('-low')).throughput.tokens_per_second, 300);
});

test('legacy per-model rates are retained without inventing family weights', () => {
    const legacy = variant('claude-sonnet-4-6-high', 100, 1, 0, { tokens_per_second: 100, timed_generations: 1 });
    assert.equal(groupModelTokenStatsByFamily([legacy], () => 0)[0].throughput.tokens_per_second, 100);
    assert.equal(groupModelTokenStatsByFamily([legacy, { ...legacy, model: 'claude-sonnet-4-6-low' }], () => 0)[0].throughput, null);
});

test('family pricing accepts versioned names while preserving specific overrides and boundaries', () => {
    const mini = { input: 0.15, output: 0.6, cached: 0.075 };
    const rules = { 'gemini-flash': pricing, 'gemini-2.0-flash': mini, 'gpt-4o': pricing, 'gpt-4o-mini': mini, default: mini };
    assert.equal(resolveModelPricing('Google/gemini-3.8-flash-high', rules, mini), pricing);
    assert.equal(resolveModelPricing('gemini-2.0-flash-high', rules, pricing), mini);
    assert.equal(resolveModelPricing('gpt-4o-mini-2024-07-18', rules, pricing), mini);
    assert.equal(resolveModelPricing('notgemini-3.8-flash', rules, pricing), mini);
});

test('thinking tiers share one trend series and retain token totals', () => {
    const { transformedTrend, displayModelKeys } = aggregateModelTrendsByFamily([
        { period: '2026-11-24', model_data: { 'gemini-3.8-flash-high': 100, 'gemini-3.8-flash-low': 900, 'claude-sonnet-4-6': 200 } },
    ], 1, '__other_models__');
    assert.deepEqual(displayModelKeys, ['gemini-3.8-flash', '__other_models__']);
    assert.equal(transformedTrend[0]['gemini-3.8-flash'], 1000);
    assert.equal(transformedTrend[0].__other_models__, 200);
});
