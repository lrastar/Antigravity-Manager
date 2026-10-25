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

export function ModelThroughputBadge({ models }: { models: { model: string; throughput?: TokenThroughputSummary | null }[] }) {
    const { t } = useTranslation();
    return (
        <details className="relative text-xs">
            <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 rounded-xl border border-emerald-200 dark:border-emerald-800/50 bg-emerald-50 dark:bg-emerald-950/20">
                <Gauge className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
                <span>{t('token_stats.model_speed', '模型速度')}</span><span className="font-mono text-gray-500">{models.length}</span>
            </summary>
            <div className="fixed left-4 right-4 top-32 sm:absolute sm:left-auto sm:right-0 sm:top-full z-40 mt-2 sm:w-96 rounded-xl border border-gray-200 dark:border-base-300 bg-white dark:bg-base-100 p-3 shadow-lg">
                <p className="mb-2 text-gray-500">{t('token_stats.model_speed_hint', '按所选时间范围统计；无可靠耗时的记录不计入速度。')}</p>
                <div className="max-h-64 overflow-y-auto">
                    {models.length === 0 && <p className="py-3 text-gray-500">{t('token_stats.no_data', '暂无数据')}</p>}
                    {models.map(model => <div key={model.model} className="flex items-center justify-between gap-3 border-t border-gray-100 dark:border-base-200 py-2">
                        <span className="min-w-0 break-all font-mono">{model.model}</span>
                        <span className="shrink-0 text-right"><strong className="block font-mono text-emerald-600 dark:text-emerald-400">{formatTokenSpeed(model.throughput?.tokens_per_second)}</strong><span className="text-[10px] text-gray-500">{t('token_stats.throughput_samples', '{{count}} 个有效样本', {count:model.throughput?.timed_generations ?? 0})}</span></span>
                    </div>)}
                </div>
            </div>
        </details>
    );
}
