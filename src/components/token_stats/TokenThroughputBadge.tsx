import { useTranslation } from 'react-i18next';
import { Gauge } from 'lucide-react';

export interface TokenThroughputSummary {
    tokens_per_second: number | null;
    timed_generations: number;
}

export function formatTokenSpeed(value?: number | null): string {
    return typeof value === 'number' && Number.isFinite(value) && value >= 0
        ? `${value.toFixed(1)} tok/s`
        : '—';
}

export function TokenThroughputBadge({ throughput }: { throughput?: TokenThroughputSummary | null }) {
    const { t } = useTranslation();
    return (
        <div
            className="flex items-center gap-2 px-3 py-1.5 rounded-xl border border-emerald-200 dark:border-emerald-800/50 bg-emerald-50 dark:bg-emerald-950/20 text-xs"
            title={t('token_stats.throughput_hint', '仅统计有可靠耗时的记录：输出 Token 总量 ÷ 生成耗时总和，包含首字等待；思考 Token 已包含在输出中，不重复计算。')}
        >
            <Gauge className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
            <span className="text-gray-600 dark:text-gray-300">{t('token_stats.throughput', '平均输出速度')}</span>
            <strong className="font-mono text-emerald-700 dark:text-emerald-300">
                {throughput?.tokens_per_second != null
                    ? formatTokenSpeed(throughput.tokens_per_second)
                    : t('token_stats.unavailable', '不可用')}
            </strong>
            <span className="text-gray-400">{t('token_stats.throughput_samples', '{{count}} 个有效样本', { count: throughput?.timed_generations || 0 })}</span>
        </div>
    );
}
