export type TokenStatsSource = 'all' | 'proxy' | 'antigravity';

export interface TokenUsageMetrics {
    inputTokens: number;
    outputTokens: number;
    cachedTokens: number;
    totalTokens: number;
    requestCount: number;
}

type UsageRecord = {
    total_input_tokens?: number;
    total_output_tokens?: number;
    total_cached_tokens?: number;
    total_tokens?: number;
    request_count?: number;
    total_requests?: number;
    native_generation_count?: number;
    total_native_generations?: number;
    proxy_input_tokens?: number;
    proxy_output_tokens?: number;
    proxy_cached_tokens?: number;
    proxy_total_tokens?: number;
};

/** Pick fields from one already-filtered statistics response for consistent UI metrics. */
export function getTokenUsageMetrics(record: UsageRecord, source: TokenStatsSource): TokenUsageMetrics {
    return {
        inputTokens: record.total_input_tokens ?? 0,
        outputTokens: record.total_output_tokens ?? 0,
        cachedTokens: record.total_cached_tokens ?? 0,
        totalTokens: record.total_tokens ?? 0,
        requestCount: source === 'antigravity'
            ? (record.native_generation_count ?? record.total_native_generations ?? 0)
            : source === 'all'
                ? (record.request_count ?? record.total_requests ?? 0) + (record.native_generation_count ?? record.total_native_generations ?? 0)
                : (record.request_count ?? record.total_requests ?? 0),
    };
}

export interface TokenPricing {
    input: number;
    output: number;
    cached: number;
}

/** Prefer the most specific configured model before a family or default price. */
export function resolveModelPricing(model: string, pricing: Record<string, TokenPricing>, fallback: TokenPricing): TokenPricing {
    const normalize = (name: string) => name.toLowerCase().trim().replace(/\./g, '-');
    const key = normalize(model);
    const entries = Object.entries(pricing)
        .filter(([pattern]) => pattern !== 'default')
        .map(([pattern, rule]) => [normalize(pattern), rule] as const)
        .sort(([a], [b]) => b.length - a.length);
    const exact = entries.find(([pattern]) => key === pattern);
    if (exact) return exact[1];
    // Model identifiers can have provider prefixes, dates, or thinking-level suffixes.
    // Bound the family match so e.g. "o1" does not match an unrelated identifier.
    const match = entries.find(([pattern]) => {
        const escaped = pattern.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        return new RegExp(`(?:^|[/.:_-])${escaped}(?:$|[-:@/\\[])`).test(key);
    });
    return match?.[1] ?? pricing.default ?? fallback;
}

export function estimateTokenCost(metrics: Pick<TokenUsageMetrics, 'inputTokens' | 'outputTokens' | 'cachedTokens'>, pricing: TokenPricing): number {
    const uncachedInput = Math.max(0, metrics.inputTokens - metrics.cachedTokens);
    return (uncachedInput * pricing.input + metrics.cachedTokens * pricing.cached + metrics.outputTokens * pricing.output) / 1_000_000;
}

export function getCacheHitRate(inputTokens: number, cachedTokens: number): number {
    return inputTokens > 0 ? Math.min(100, (cachedTokens / inputTokens) * 100) : 0;
}
