import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Area, AreaChart, Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { BarChart3, Database, RefreshCw, Settings2, Sparkles, Terminal, Zap } from 'lucide-react';
import { request as invoke } from '../utils/request';
import { estimateTokenCost, getCacheHitRate, getTokenUsageMetrics, resolveModelPricing, type TokenStatsSource } from '../utils/tokenStatsMetrics';
import { getStatsRangeBounds, restoreStatsDateRange, type StatsBucket, type UsageDateRange } from '../utils/tokenStatsDateRange';
import { UsageDateRangePicker } from '../components/token_stats/UsageDateRangePicker';
import { UsageSummaryCards } from '../components/token_stats/UsageSummaryCards';
import { ModelThroughputBadge, formatTokenSpeed, type TokenThroughputSummary } from '../components/token_stats/TokenThroughputBadge';
import { CyberCockpitView } from '../components/token_stats/CyberCockpitView';
import { AppleBentoView } from '../components/token_stats/AppleBentoView';
import { PricingModal } from '../components/token_stats/PricingModal';
import { NativeTokenModal } from '../components/token_stats/NativeTokenModal';
import type { DayUsagePoint } from '../components/token_stats/ActivityHeatmapCard';
import type { HourlyUsagePoint } from '../components/token_stats/HourlyTrendBarCard';
import type { ModelShareItem } from '../components/token_stats/TopModelsShareCard';

export type { TokenStatsSource } from '../utils/tokenStatsMetrics';

export interface TokenStatsAggregated {
    period: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    native_generation_count?: number;
    uncached_input_tokens?: number;
}

export interface AccountTokenStats {
    account_email: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    native_generation_count?: number;
    proxy_input_tokens?: number;
    proxy_output_tokens?: number;
    proxy_cached_tokens?: number;
    proxy_total_tokens?: number;
}

export interface ModelTokenStats {
    model: string;
    throughput?: TokenThroughputSummary | null;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    native_generation_count?: number;
    proxy_input_tokens?: number;
    proxy_output_tokens?: number;
    proxy_cached_tokens?: number;
    proxy_total_tokens?: number;
}

export interface ModelTrendPoint {
    period: string;
    model_data: Record<string, number>;
}

export interface AccountTrendPoint {
    period: string;
    account_data: Record<string, number>;
}

export interface TokenStatsSummary {
    throughput?: TokenThroughputSummary | null;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    total_requests: number;
    total_native_generations?: number;
    unique_accounts: number;
    unassigned_tokens?: number;
    proxy_input_tokens?: number;
    proxy_output_tokens?: number;
    proxy_cached_tokens?: number;
    proxy_total_tokens?: number;
}

export interface ModelPricingRule {
    input: number; // $ per 1M tokens
    output: number; // $ per 1M tokens
    cached: number; // $ per 1M tokens
}

export const DEFAULT_PRICING: Record<string, ModelPricingRule> = {
    // Google Gemini 3.x & 2.x
    'gemini-3.8-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.7-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.6-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.5-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-2.5-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-2.0-flash': { input: 0.10, output: 0.40, cached: 0.025 },
    'gemini-3.1-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    'gemini-3-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    'gemini-2.5-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    // Anthropic Claude 4.x & 3.x
    'claude-sonnet-4-6': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-sonnet-4-5': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-3-7-sonnet': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-3-5-sonnet': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-opus-4-6': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-opus-4-5': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-3-opus': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-haiku-4': { input: 0.80, output: 4.00, cached: 0.08 },
    'claude-3-5-haiku': { input: 0.80, output: 4.00, cached: 0.08 },
    // OpenAI Models
    'gpt-4o': { input: 2.50, output: 10.00, cached: 1.25 },
    'gpt-4o-mini': { input: 0.15, output: 0.60, cached: 0.075 },
    'o3-mini': { input: 1.10, output: 4.40, cached: 0.55 },
    'o1': { input: 15.00, output: 60.00, cached: 7.50 },
    // Default Fallback
    'default': { input: 1.00, output: 5.00, cached: 0.10 },
};

const PRICING_STORAGE_KEY = 'antigravity_model_pricing_v2026';
const DASHBOARD_MODE_KEY = 'antigravity_token_stats_dashboard_mode';
const DATE_RANGE_KEY = 'antigravity_token_stats_date_range';
const CHART_TYPE_KEY = 'antigravity_token_stats_chart_type';
const METRIC_TYPE_KEY = 'antigravity_token_stats_metric_type';
type DashboardMode = 'cockpit' | 'insights' | 'classic';
type MetricType = 'tokens' | 'cost';
const COLORS = ['#d97757', '#4f85e8', '#10a37f', '#a855f7', '#f59e0b', '#71717a'];
const SOURCE: TokenStatsSource = 'all';

interface DashboardResponse {
    summary: TokenStatsSummary;
    models: ModelTokenStats[];
    timeline: TokenStatsAggregated[];
    model_trend: ModelTrendPoint[];
}
interface DashboardSnapshot extends DashboardResponse {
    filterKey: string;
    bucket: StatsBucket;
    heatmap: DayUsagePoint[];
    hourly: HourlyUsagePoint[];
}

function readPreference(key: string) {
    try { return localStorage.getItem(key); } catch { return null; }
}
function savePreference(key: string, value: string) {
    try { localStorage.setItem(key, value); } catch { /* Session state remains usable. */ }
}
const formatNumber = (value: number) => value >= 1e9 ? `${(value / 1e9).toFixed(1)}B` : value >= 1e6 ? `${(value / 1e6).toFixed(1)}M` : value >= 1e3 ? `${(value / 1e3).toFixed(1)}K` : value.toLocaleString();
const formatCurrency = (value: number) => value === 0 ? '$0' : `$${value.toLocaleString(undefined, { minimumFractionDigits: value < 1 ? 3 : 2, maximumFractionDigits: value < 1 ? 3 : 2 })}`;

export default function TokenStats() {
    const { t } = useTranslation();
    const [range, setRange] = useState<UsageDateRange>(() => restoreStatsDateRange(readPreference(DATE_RANGE_KEY)));
    const [dashboardMode, setDashboardMode] = useState<DashboardMode>(() => {
        const saved = readPreference(DASHBOARD_MODE_KEY);
        return saved === 'cockpit' ? 'cockpit' : saved === 'insights' || saved === 'apple' ? 'insights' : 'classic';
    });
    const [metricType, setMetricType] = useState<MetricType>(() => readPreference(METRIC_TYPE_KEY) === 'cost' ? 'cost' : 'tokens');
    const [chartType, setChartType] = useState<'bar' | 'area'>(() => readPreference(CHART_TYPE_KEY) === 'area' ? 'area' : 'bar');
    const [pricing, setPricing] = useState<Record<string, ModelPricingRule>>(() => {
        try { return JSON.parse(readPreference(PRICING_STORAGE_KEY) || 'null') || DEFAULT_PRICING; } catch { return DEFAULT_PRICING; }
    });
    const [showPricingModal, setShowPricingModal] = useState(false);
    const [showNativeModal, setShowNativeModal] = useState(false);
    const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const filterKey = JSON.stringify(range);
    const currentFilterRef = useRef(filterKey);
    currentFilterRef.current = filterKey;
    const requestIdRef = useRef(0);
    const pollingRef = useRef(false);

    const fetchData = useCallback(async (silent = false) => {
        const requestId = ++requestIdRef.current;
        const isCurrent = () => requestIdRef.current === requestId && currentFilterRef.current === filterKey;
        if (!silent) setLoading(true);
        try {
            const bounds = getStatsRangeBounds(range);
            const [dashboard, days, hours] = await Promise.all([
                invoke<DashboardResponse>('get_token_stats_dashboard', bounds),
                invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 91, source: SOURCE }),
                invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24, source: SOURCE }),
            ]);
            if (!isCurrent()) return;
            setSnapshot({ ...dashboard, filterKey, bucket: bounds.bucket,
                timeline: dashboard.timeline.map(point => ({ ...point, uncached_input_tokens: Math.max(0, point.total_input_tokens - point.total_cached_tokens) })),
                heatmap: days.map(point => ({ date: point.period, total_tokens: point.total_tokens, request_count: getTokenUsageMetrics(point, SOURCE).requestCount, total_input_tokens: point.total_input_tokens, total_output_tokens: point.total_output_tokens, total_cached_tokens: point.total_cached_tokens })),
                hourly: hours.map(point => ({ hour: point.period, total_tokens: point.total_tokens, request_count: getTokenUsageMetrics(point, SOURCE).requestCount, input_tokens: point.total_input_tokens, output_tokens: point.total_output_tokens, cached_tokens: point.total_cached_tokens })),
            });
            setError(null);
        } catch (cause) {
            if (isCurrent()) setError(String(cause));
        } finally {
            if (isCurrent()) setLoading(false);
        }
    }, [filterKey, range]);

    useEffect(() => {
        void fetchData();
        const timer = window.setInterval(() => {
            if (document.hidden || pollingRef.current) return;
            pollingRef.current = true;
            void fetchData(true).finally(() => { pollingRef.current = false; });
        }, 30_000);
        return () => { window.clearInterval(timer); requestIdRef.current += 1; };
    }, [fetchData]);

    // Never label a response from the previous date range with the new filter.
    const data = snapshot?.filterKey === filterKey ? snapshot : null;
    const models = data?.models ?? [];
    const summary = data?.summary ?? null;
    const modelShareItems = useMemo<ModelShareItem[]>(() => models.map(model => ({
        ...model,
        request_count: getTokenUsageMetrics(model, SOURCE).requestCount,
        cost: estimateTokenCost(getTokenUsageMetrics(model, SOURCE), resolveModelPricing(model.model, pricing, DEFAULT_PRICING.default)),
    })), [data, pricing]);
    const totalEstimatedCost = modelShareItems.reduce((total, model) => total + model.cost, 0);
    const cacheMetrics = { hitRate: getCacheHitRate(summary?.total_input_tokens ?? 0, summary?.total_cached_tokens ?? 0), cachedTokens: summary?.total_cached_tokens ?? 0 };
    const { series, trendData } = useMemo(() => {
        const topModels = modelShareItems.slice(0, 5).map(model => model.model);
        const otherKey = '__other_models__';
        const rates = new Map(modelShareItems.map(model => [model.model, model.total_tokens > 0 ? model.cost / model.total_tokens : 0]));
        const series = modelShareItems.length > 5 ? [...topModels, otherKey] : topModels;
        return { series, trendData: (data?.model_trend ?? []).map(point => {
            const row: Record<string, string | number> = { period: point.period };
            for (const key of series) row[key] = 0;
            for (const [model, tokens] of Object.entries(point.model_data)) {
                const key = topModels.includes(model) ? model : otherKey;
                row[key] = Number(row[key] ?? 0) + (metricType === 'cost' ? tokens * (rates.get(model) ?? 0) : tokens);
            }
            return row;
        }) };
    }, [data, modelShareItems, metricType]);
    const modelLabel = (key: string) => key === '__other_models__' ? t('token_stats.other_models', '其他模型') : key;
    const switchMetricType = (metric: MetricType) => { setMetricType(metric); savePreference(METRIC_TYPE_KEY, metric); };
    const switchChartType = (type: 'bar' | 'area') => { setChartType(type); savePreference(CHART_TYPE_KEY, type); };
    const rangeLabel = range.preset === 'custom'
        ? `${range.start.replace('T', ' ')} — ${range.end.replace('T', ' ')}`
        : t(`token_stats.range_${range.preset}`, range.preset);
    const TrendTooltip = ({ active, payload, label }: any) => active && payload?.length ? (
        <div className="rounded-xl border border-gray-200 dark:border-base-300 bg-white dark:bg-base-100 p-3 shadow-lg text-xs">
            <p className="mb-2 font-semibold">{label}</p>
            {payload.map((item: any) => <div key={item.dataKey} className="flex justify-between gap-4"><span style={{ color: item.color }}>{modelLabel(item.dataKey)}</span><span className="font-mono">{metricType === 'cost' ? formatCurrency(Number(item.value)) : formatNumber(Number(item.value))}</span></div>)}
        </div>
    ) : null;
    const axes = <><CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150,150,150,0.15)" /><XAxis dataKey="period" tick={{fontSize:11}} tickFormatter={value => data?.bucket === 'hourly' ? value.split(' ')[1] : value.slice(5)} /><YAxis tick={{fontSize:11}} tickFormatter={metricType === 'cost' ? formatCurrency : formatNumber} /><Tooltip content={<TrendTooltip />} /></>;
    const pill = 'rounded-lg px-3 py-1.5 text-xs transition-colors';

    return (
        <div className="h-full overflow-y-auto text-gray-900 dark:text-base-content">
            <div className="mx-auto max-w-7xl space-y-4 p-4 sm:p-5">
                <header className="flex flex-wrap items-center justify-between gap-4">
                    <div className="flex items-center gap-3"><Zap className="h-6 w-6 text-blue-500" /><div><h1 className="text-xl font-bold">{t('token_stats.title', 'Token 消费统计')}</h1><p className="mt-1 text-xs text-gray-500">{t('token_stats.all_usage_note', '全部用量 · 按模型查看消耗与速度')}</p></div></div>
                    <div className="flex flex-wrap items-center gap-2">
                        <div className="flex rounded-xl border border-gray-200 dark:border-base-300 bg-gray-100 dark:bg-base-200 p-1">
                            {([['classic', BarChart3, t('token_stats.usage_overview', '用量概览')], ['cockpit', Terminal, t('token_stats.mode_cockpit', '总览看板')], ['insights', Sparkles, t('token_stats.mode_insights', '深度洞察')]] as const).map(([mode, Icon, label]) => <button key={mode} className={`${pill} flex items-center gap-1 ${dashboardMode === mode ? 'bg-white dark:bg-base-100 text-blue-600 shadow-sm' : 'text-gray-500'}`} onClick={() => { setDashboardMode(mode); savePreference(DASHBOARD_MODE_KEY, mode); }}><Icon className="h-3.5 w-3.5" />{label}</button>)}
                        </div>
                        <ModelThroughputBadge models={models} />
                        <UsageDateRangePicker value={range} onChange={value => { setRange(value); savePreference(DATE_RANGE_KEY, JSON.stringify(value)); }} />
                        <button type="button" className="rounded-xl border border-gray-200 dark:border-base-300 p-2" onClick={() => setShowNativeModal(true)} aria-label={t('token_stats.collection_settings', '采集设置')} title={t('token_stats.collection_settings', '采集设置')}><Database className="h-4 w-4" /></button>
                        <button type="button" className="rounded-xl border border-gray-200 dark:border-base-300 p-2" onClick={() => setShowPricingModal(true)} aria-label={t('token_stats.custom_pricing', '自定义价格')}><Settings2 className="h-4 w-4" /></button>
                        <button type="button" disabled={loading} onClick={() => void fetchData()} className="rounded-xl bg-blue-600 p-2 text-white disabled:opacity-50" aria-label={t('common.refresh', '刷新')}><RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} /></button>
                    </div>
                </header>
                {range.preset === 'custom' && <p className="text-xs text-gray-500">{rangeLabel}</p>}
                {error && <div role="alert" className="rounded-xl bg-red-50 dark:bg-red-950/20 p-3 text-xs text-red-600">{t('token_stats.load_failed', '统计加载失败，请刷新重试')}<span className="ml-2 break-all">{error}</span></div>}
                {!data ? <div role="status" className="py-16 text-center text-sm text-gray-500">{loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}</div> : <>
                    {dashboardMode === 'classic' && <>
                        <UsageSummaryCards summary={data.summary} source={SOURCE} modelCount={models.length} estimatedCost={totalEstimatedCost} formatNumber={formatNumber} formatCurrency={formatCurrency} onConfigurePricing={() => setShowPricingModal(true)} />
                        <section className="rounded-2xl border border-gray-200 dark:border-base-200 bg-white dark:bg-base-100 p-4 sm:p-5 shadow-sm">
                            <div className="mb-3 flex flex-wrap items-center justify-between gap-3"><h2 className="text-sm font-semibold">{metricType === 'cost' ? t('token_stats.model_estimated_cost_trend', '分模型估算费用趋势') : t('token_stats.model_trend', '分模型使用趋势')}</h2><div className="flex gap-2">
                                <div className="flex rounded-xl bg-gray-100 dark:bg-base-200 p-1">{(['bar','area'] as const).map(type => <button key={type} onClick={() => switchChartType(type)} className={`${pill} ${type === chartType ? 'bg-white dark:bg-base-100 shadow-sm' : ''}`}>{type === 'bar' ? t('token_stats.bar_chart', '柱状') : t('token_stats.area_chart', '面积')}</button>)}</div>
                                <div className="flex rounded-xl bg-gray-100 dark:bg-base-200 p-1">{(['tokens','cost'] as const).map(metric => <button key={metric} onClick={() => switchMetricType(metric)} className={`${pill} ${metric === metricType ? 'bg-white dark:bg-base-100 shadow-sm' : ''}`}>{metric === 'tokens' ? 'Token' : t('token_stats.estimated_cost', '估算费用')}</button>)}</div>
                            </div></div>
                            {metricType === 'cost' && <p className="mb-3 text-xs text-gray-500">{t('token_stats.cost_trend_note', '费用趋势按所选时段平均 Token 单价分摊估算。')}</p>}
                            <div className="h-64">{trendData.length ? <ResponsiveContainer width="100%" height="100%">{chartType === 'bar' ? <BarChart data={trendData}>{axes}{series.map((key,index) => <Bar key={key} dataKey={key} stackId="usage" fill={COLORS[index % COLORS.length]} />)}</BarChart> : <AreaChart data={trendData}>{axes}{series.map((key,index) => <Area key={key} type="monotone" dataKey={key} stackId="usage" stroke={COLORS[index % COLORS.length]} fill={COLORS[index % COLORS.length]} fillOpacity={0.15} />)}</AreaChart>}</ResponsiveContainer> : <div className="flex h-full items-center justify-center text-sm text-gray-500">{t('token_stats.no_data', '暂无数据')}</div>}</div>
                            <div className="mt-3 flex flex-wrap gap-3 text-xs text-gray-500">{series.map((key,index) => <span key={key} className="flex items-center gap-1.5"><span className="h-2 w-2 rounded-full" style={{background:COLORS[index % COLORS.length]}} />{modelLabel(key)}</span>)}</div>
                        </section>
                    </>}
                    {dashboardMode === 'cockpit' && <div className="overflow-x-auto"><div className="h-[calc(100vh-180px)] min-h-[620px] min-w-[960px]"><CyberCockpitView summary={summary} source={SOURCE} hourlyData={data.hourly} timelineData={data.timeline} modelShareItems={modelShareItems} rangeLabel={rangeLabel} totalEstimatedCost={totalEstimatedCost} cacheMetrics={cacheMetrics} formatNumber={formatNumber} formatCurrency={formatCurrency} setShowPricingModal={setShowPricingModal} /></div></div>}
                    {dashboardMode === 'insights' && <AppleBentoView summary={summary} source={SOURCE} dailyData={data.heatmap} hourlyData={data.hourly} timelineData={data.timeline} modelShareItems={modelShareItems} displayModelTrendData={trendData} allModels={series} metricType={metricType} chartType={chartType} timeRange={data.bucket} totalEstimatedCost={totalEstimatedCost} cacheMetrics={cacheMetrics} formatNumber={formatNumber} formatCurrency={formatCurrency} switchChartType={switchChartType} switchMetricType={switchMetricType} setShowPricingModal={setShowPricingModal} customTooltip={TrendTooltip} />}
                    {dashboardMode !== 'cockpit' && <section className="rounded-2xl border border-gray-200 dark:border-base-200 bg-white dark:bg-base-100 p-4 shadow-sm">
                        <h2 className="mb-3 text-sm font-semibold">{t('token_stats.model_details', '分模型详细统计')}</h2>
                        <div className="overflow-x-auto"><table className="w-full whitespace-nowrap text-xs"><thead><tr className="border-b border-gray-200 dark:border-base-200 text-gray-500">{[t('token_stats.model','模型'),t('token_stats.calls','调用数'),t('token_stats.input','输入'),t('token_stats.output','输出'),t('token_stats.cached_tokens_title','缓存命中'),t('token_stats.total','合计'),t('token_stats.model_speed','模型速度'),t('token_stats.estimated_cost','估算费用')].map((label,index) => <th key={label} className={`px-3 py-3 font-medium ${index === 0 ? 'text-left' : 'text-right'}`}>{label}</th>)}</tr></thead><tbody>
                            {modelShareItems.map(model => <tr key={model.model} className="border-b border-gray-100 dark:border-base-200 last:border-0"><td className="px-3 py-3 font-mono">{model.model}</td>{[model.request_count,model.total_input_tokens,model.total_output_tokens,model.total_cached_tokens,model.total_tokens].map((value,index) => <td key={index} className="px-3 py-3 text-right font-mono" title={value.toLocaleString()}>{formatNumber(value)}</td>)}<td className="px-3 py-3 text-right font-mono text-emerald-600" title={t('token_stats.throughput_samples', '{{count}} 个有效样本', {count:model.throughput?.timed_generations ?? 0})}>{formatTokenSpeed(model.throughput?.tokens_per_second)}</td><td className="px-3 py-3 text-right font-mono">{formatCurrency(model.cost)}</td></tr>)}
                            {!modelShareItems.length && <tr><td colSpan={8} className="py-8 text-center text-gray-500">{t('token_stats.no_data', '暂无数据')}</td></tr>}
                        </tbody></table></div>
                    </section>}
                </>}
                <PricingModal isOpen={showPricingModal} onClose={() => setShowPricingModal(false)} currentPricing={pricing} onSave={rules => { setPricing(rules); savePreference(PRICING_STORAGE_KEY, JSON.stringify(rules)); setShowPricingModal(false); }} />
                <NativeTokenModal isOpen={showNativeModal} onClose={() => setShowNativeModal(false)} onDataChanged={() => void fetchData()} />
            </div>
        </div>
    );
}
