import { Activity, ChevronDown, Database, DollarSign, Settings2, Zap } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { TokenStatsSummary } from '../../pages/TokenStats';
import { getCacheHitRate, getTokenUsageMetrics, type TokenStatsSource } from '../../utils/tokenStatsMetrics';

interface UsageSummaryCardsProps {
    summary: TokenStatsSummary;
    source: TokenStatsSource;
    modelCount: number;
    estimatedCost: number;
    formatNumber: (value: number) => string;
    formatCurrency: (value: number) => string;
    onConfigurePricing: () => void;
}

export function UsageSummaryCards({ summary, source, modelCount, estimatedCost, formatNumber, formatCurrency, onConfigurePricing }: UsageSummaryCardsProps) {
    const { t } = useTranslation();
    const usage = getTokenUsageMetrics(summary, source);
    const cardClass = 'min-w-0 rounded-2xl border border-gray-200/80 dark:border-base-200 bg-white dark:bg-base-100 p-4 shadow-sm';
    const labelClass = 'flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400';
    const valueClass = 'mt-3 text-2xl sm:text-3xl font-semibold font-mono tracking-tight text-gray-900 dark:text-base-content';
    const noteClass = 'mt-2 text-xs text-gray-500 dark:text-gray-400';

    return (
        <section aria-label={t('token_stats.usage_overview', '用量概览')} className="space-y-3">
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3.5">
                <div className={cardClass}>
                    <div className="flex items-center justify-between gap-2">
                        <span className={labelClass}><DollarSign className="h-4 w-4 text-emerald-500" />{t('token_stats.estimated_cost', '估算费用')}</span>
                        <button type="button" onClick={onConfigurePricing} className="p-1 rounded-md text-gray-400 hover:text-emerald-600 focus-visible:outline focus-visible:outline-2 focus-visible:outline-emerald-500" aria-label={t('token_stats.custom_pricing', '自定义单价')}>
                            <Settings2 className="h-4 w-4" />
                        </button>
                    </div>
                    <div className={valueClass}>{formatCurrency(estimatedCost)}</div>
                    <p className={noteClass}>{t('token_stats.estimated_cost_note', '按当前配置单价估算，不代表实际账单')}</p>
                </div>
                <div className={cardClass}>
                    <div className={labelClass}><Activity className="h-4 w-4 text-blue-500" />{t('token_stats.calls', '调用数')}</div>
                    <div className={valueClass} title={usage.requestCount.toLocaleString()}>{formatNumber(usage.requestCount)}</div>
                    <p className={noteClass}>
                        {t('token_stats.calls_note', '所选时段全部调用')}
                    </p>
                </div>
                <div className={cardClass}>
                    <div className={labelClass}><Zap className="h-4 w-4 text-amber-500" />{t('token_stats.total_tokens', '总 Token')}</div>
                    <div className={valueClass} title={usage.totalTokens.toLocaleString()}>{formatNumber(usage.totalTokens)}</div>
                    <p className={noteClass}>{t('token_stats.total_tokens_note', '输入（含缓存）+ 输出')}</p>
                </div>
                <div className={cardClass}>
                    <div className={labelClass}><Database className="h-4 w-4 text-cyan-500" />{t('token_stats.cached_tokens_title', '缓存命中')}</div>
                    <div className={valueClass}>{getCacheHitRate(usage.inputTokens, usage.cachedTokens).toFixed(1)}%</div>
                    <p className={noteClass}>{t('token_stats.cached_tokens_count', '{{tokens}} Tokens 命中', { tokens: formatNumber(usage.cachedTokens) })}</p>
                </div>
            </div>
            <details className="group rounded-xl border border-gray-200/80 dark:border-base-200 bg-white dark:bg-base-100">
                <summary className="flex cursor-pointer items-center gap-2 px-4 py-2.5 text-xs text-gray-600 dark:text-gray-300 list-none">
                    <ChevronDown className="h-3.5 w-3.5 transition-transform group-open:rotate-180" />
                    {t('token_stats.more_metrics', '更多指标')}
                </summary>
                <dl className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-4 border-t border-gray-100 dark:border-base-200 px-4 py-3 text-xs">
                    {[
                        [t('token_stats.input_tokens', '输入 Token'), usage.inputTokens],
                        [t('token_stats.uncached_input', '未缓存输入'), Math.max(0, usage.inputTokens - usage.cachedTokens)],
                        [t('token_stats.output_tokens', '输出 Token'), usage.outputTokens],
                        [t('token_stats.cached_tokens_title', '缓存命中'), usage.cachedTokens],
                        [t('token_stats.models_used', '使用模型'), modelCount],
                    ].map(([label, value]) => (
                        <div key={label}>
                            <dt className="text-gray-500 dark:text-gray-400">{label}</dt>
                            <dd className="mt-1 font-mono font-semibold text-gray-900 dark:text-base-content">{Number(value).toLocaleString()}</dd>
                        </div>
                    ))}
                </dl>
                <p className="px-4 pb-3 text-xs text-gray-500 dark:text-gray-400">{t('token_stats.cache_hit_note', '命中率 = 缓存命中 ÷ 输入 Token；输入已包含缓存，不重复累计。')}</p>
            </details>
        </section>
    );
}
