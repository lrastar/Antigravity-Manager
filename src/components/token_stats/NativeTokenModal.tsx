import React, { useState, useEffect, useCallback, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import {
    X,
    RefreshCw,
    Search,
    Cpu,
    UserCheck,
    UserX,
    Layers,
    ChevronLeft,
    ChevronRight,
} from 'lucide-react';
import { request as invoke } from '../../utils/request';
import { formatTokenSpeed } from './TokenThroughputBadge';

export interface NativeCollectorStatus {
    enabled: boolean;
    baseline_established_at: number | null;
    enabled_since?: number | null;
    last_scanned_at: number | null;
    total_native_records: number;
    unassigned_records: number;
    unassigned_tokens: number;
    last_error: string | null;
    last_error_at?: number | null;
    active_observation_account?: string | null;
}

export interface NativeUsageRecord {
    id: number;
    record_key: string;
    session_id: string;
    conversation_id?: string;
    response_id: string | null;
    model: string;
    model_raw?: string;
    model_normalized?: string;
    uncached_input_tokens?: number;
    input_tokens: number;
    output_tokens: number;
    cached_tokens: number;
    cache_write_tokens?: number;
    reasoning_tokens: number;
    total_tokens: number;
    duration_ms?: number | null;
    tokens_per_second?: number | null;
    timestamp: number;
    account_email: string | null;
    attribution_type: string;
    attribution_method?: string;
    created_at?: number;
}

export interface NativeRecordsPage {
    records: NativeUsageRecord[];
    total_count: number;
    unassigned_count?: number;
    page?: number;
    page_size?: number;
    total_pages?: number;
}

interface AccountItem {
    id: string;
    email: string;
}

interface NativeTokenModalProps {
    isOpen: boolean;
    onClose: () => void;
    onDataChanged?: () => void;
}

const formatNumber = (num: number): string => {
    if (num >= 1_000_000_000) return `${(num / 1_000_000_000).toFixed(1)}B`;
    if (num >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`;
    if (num >= 1_000) return `${(num / 1_000).toFixed(1)}K`;
    return num.toString();
};

const formatTime = (ts: number): string => {
    if (!ts || ts === 0) return '-';
    // ts may be in seconds or milliseconds
    const ms = ts < 10000000000 ? ts * 1000 : ts;
    const d = new Date(ms);
    return d.toLocaleString(undefined, {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
    });
};

export const NativeTokenModal: React.FC<NativeTokenModalProps> = ({
    isOpen,
    onClose,
    onDataChanged,
}) => {
    const { t } = useTranslation();

    const [status, setStatus] = useState<NativeCollectorStatus | null>(null);
    const [statusLoading, setStatusLoading] = useState(false);
    const [togglingEnabled, setTogglingEnabled] = useState(false);
    const [refreshing, setRefreshing] = useState(false);

    // Records list state
    const [recordsPage, setRecordsPage] = useState<NativeRecordsPage>({
        records: [],
        total_count: 0,
        page: 1,
        page_size: 20,
        total_pages: 0,
    });
    const [page, setPage] = useState(1);
    const [search, setSearch] = useState('');
    const [searchInput, setSearchInput] = useState('');
    const [loadingRecords, setLoadingRecords] = useState(false);
    const isOpenRef = useRef(isOpen);
    isOpenRef.current = isOpen;
    const recordsRequestRef = useRef(0);
    const statusRequestRef = useRef(0);
    const recordsInFlightRef = useRef(false);
    const statusInFlightRef = useRef(false);
    const recordsPollIdRef = useRef(0);
    const statusPollIdRef = useRef(0);
    const pageRef = useRef(page);
    const searchRef = useRef(search);
    pageRef.current = page;
    searchRef.current = search;

    // Accounts for attribution
    const [accounts, setAccounts] = useState<AccountItem[]>([]);

    // Batch selection
    const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
    const [batchAccount, setBatchAccount] = useState('');
    const [batchOperating, setBatchOperating] = useState(false);

    // Fetch accounts
    const loadAccounts = useCallback(async () => {
        try {
            const res = await invoke<any[]>('list_accounts');
            if (Array.isArray(res)) {
                setAccounts(res.map(a => ({ id: a.id || a.account_id || '', email: a.email || a.account_email || '' })).filter(a => !!a.email));
            }
        } catch (e) {
            console.error('Failed to load accounts for attribution:', e);
        }
    }, []);

    // Fetch collector status
    const loadStatus = useCallback(async (silent = false) => {
        const requestId = ++statusRequestRef.current;
        if (!silent) setStatusLoading(true);
        try {
            const res = await invoke<NativeCollectorStatus>('get_native_collector_status');
            if (isOpenRef.current && requestId === statusRequestRef.current) setStatus(res);
        } catch (e) {
            console.error('Failed to load native collector status:', e);
        } finally {
            if (requestId === statusRequestRef.current) setStatusLoading(false);
        }
    }, []);

    // Fetch records page
    const loadRecords = useCallback(async (pageNum: number, searchKeyword: string, silent = false) => {
        if (
            !isOpenRef.current ||
            pageNum !== pageRef.current ||
            searchKeyword !== searchRef.current
        ) return;
        const requestId = ++recordsRequestRef.current;
        if (!silent) setLoadingRecords(true);
        try {
            const pageSize = 20;
            const offset = (pageNum - 1) * pageSize;
            const res = await invoke<NativeRecordsPage>('list_native_token_records', {
                limit: pageSize,
                offset,
                search: searchKeyword.trim() ? searchKeyword.trim() : null,
            });
            if (
                isOpenRef.current &&
                requestId === recordsRequestRef.current &&
                pageNum === pageRef.current &&
                searchKeyword === searchRef.current
            ) {
                setRecordsPage(res);
                const visibleKeys = new Set((res.records || []).map(record => record.record_key));
                setSelectedKeys(previous => new Set(Array.from(previous).filter(key => visibleKeys.has(key))));
            }
        } catch (e) {
            console.error('Failed to list native token records:', e);
        } finally {
            if (requestId === recordsRequestRef.current) setLoadingRecords(false);
        }
    }, []);

    useEffect(() => {
        if (isOpen) {
            isOpenRef.current = true;
            loadAccounts();
            loadStatus();
            loadRecords(pageRef.current, searchRef.current);
        } else {
            isOpenRef.current = false;
            recordsRequestRef.current += 1;
            statusRequestRef.current += 1;
            recordsInFlightRef.current = false;
            statusInFlightRef.current = false;
            recordsPollIdRef.current += 1;
            statusPollIdRef.current += 1;
            setLoadingRecords(false);
            setStatusLoading(false);
        }
        return () => {
            // Invalidate pending responses on close and unmount.
            isOpenRef.current = false;
            recordsRequestRef.current += 1;
            statusRequestRef.current += 1;
            recordsInFlightRef.current = false;
            statusInFlightRef.current = false;
            recordsPollIdRef.current += 1;
            statusPollIdRef.current += 1;
        };
    }, [isOpen, loadAccounts, loadStatus, loadRecords]);

    useEffect(() => {
        if (!isOpen) return;
        const timer = window.setInterval(() => {
            if (!statusInFlightRef.current) {
                statusInFlightRef.current = true;
                const pollId = ++statusPollIdRef.current;
                void loadStatus(true).finally(() => {
                    if (pollId === statusPollIdRef.current) statusInFlightRef.current = false;
                });
            }
            if (!recordsInFlightRef.current) {
                recordsInFlightRef.current = true;
                const pollId = ++recordsPollIdRef.current;
                void loadRecords(pageRef.current, searchRef.current, true).finally(() => {
                    if (pollId === recordsPollIdRef.current) recordsInFlightRef.current = false;
                });
            }
        }, 30_000);
        return () => window.clearInterval(timer);
    }, [isOpen, loadStatus, loadRecords]);

    // Handle search debounce or submit
    const handleSearchSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        setSearch(searchInput);
        setPage(1);
        setSelectedKeys(new Set());
        pageRef.current = 1;
        searchRef.current = searchInput;
        loadRecords(1, searchInput);
    };

    // Toggle enabled
    const handleToggleEnabled = async () => {
        if (!status || togglingEnabled) return;
        setTogglingEnabled(true);
        try {
            await invoke('set_native_collector_enabled', { enabled: !status.enabled });
            await loadStatus();
            await loadRecords(page, search);
            onDataChanged?.();
        } catch (e) {
            console.error('Failed to toggle native collector:', e);
        } finally {
            setTogglingEnabled(false);
        }
    };

    // Immediate scan
    const handleScanNow = async () => {
        if (refreshing) return;
        setRefreshing(true);
        try {
            const updated = await invoke<NativeCollectorStatus>('refresh_native_token_stats');
            setStatus(updated);
            await loadRecords(page, search);
            onDataChanged?.();
        } catch (e) {
            console.error('Failed to scan native tokens:', e);
        } finally {
            setRefreshing(false);
        }
    };

    // Single assign
    const handleAssignRecord = async (recordKey: string, accountEmail: string) => {
        try {
            if (accountEmail) {
                await invoke('assign_native_usage_account', { recordKey, accountEmail });
            } else {
                await invoke('unassign_native_usage_account', { recordKey });
            }
            await loadStatus();
            await loadRecords(page, search);
            onDataChanged?.();
        } catch (e) {
            console.error('Failed to update attribution for record:', e);
        }
    };

    // Batch select all on page
    const toggleSelectAll = () => {
        if (selectedKeys.size === recordsPage.records.length && recordsPage.records.length > 0) {
            setSelectedKeys(new Set());
        } else {
            const next = new Set<string>();
            recordsPage.records.forEach(r => next.add(r.record_key));
            setSelectedKeys(next);
        }
    };

    const toggleSelectKey = (key: string) => {
        const next = new Set(selectedKeys);
        if (next.has(key)) {
            next.delete(key);
        } else {
            next.add(key);
        }
        setSelectedKeys(next);
    };

    // Batch assign
    const handleBatchAssign = async () => {
        if (selectedKeys.size === 0 || !batchAccount || batchOperating) return;
        setBatchOperating(true);
        try {
            await invoke('batch_assign_native_usage', {
                recordKeys: Array.from(selectedKeys),
                accountEmail: batchAccount,
            });
            await loadStatus();
            await loadRecords(page, search);
            setSelectedKeys(new Set());
            onDataChanged?.();
        } catch (e) {
            console.error('Batch assign failed:', e);
        } finally {
            setBatchOperating(false);
        }
    };

    // Batch unassign
    const handleBatchUnassign = async () => {
        if (selectedKeys.size === 0 || batchOperating) return;
        setBatchOperating(true);
        try {
            await invoke('batch_unassign_native_usage', {
                recordKeys: Array.from(selectedKeys),
            });
            await loadStatus();
            await loadRecords(page, search);
            setSelectedKeys(new Set());
            onDataChanged?.();
        } catch (e) {
            console.error('Batch unassign failed:', e);
        } finally {
            setBatchOperating(false);
        }
    };

    const pageSize = recordsPage.page_size || 20;
    const totalPages = Math.max(
        1,
        recordsPage.total_pages || Math.ceil((recordsPage.total_count || 0) / pageSize)
    );
    const currentPage = recordsPage.page || page;

    const handlePrevPage = () => {
        if (page > 1) {
            const nextP = page - 1;
            setPage(nextP);
            setSelectedKeys(new Set());
            pageRef.current = nextP;
            loadRecords(nextP, search);
        }
    };

    const handleNextPage = () => {
        if (page < totalPages) {
            const nextP = page + 1;
            setPage(nextP);
            setSelectedKeys(new Set());
            pageRef.current = nextP;
            loadRecords(nextP, search);
        }
    };

    if (!isOpen) return null;

    return createPortal(
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-3 sm:p-6 overflow-hidden">
            {/* Backdrop */}
            <div
                className="fixed inset-0 bg-black/60 dark:bg-black/80 backdrop-blur-md transition-opacity"
                onClick={onClose}
            />

            {/* Modal Card */}
            <div className="relative z-10 bg-white dark:bg-[#181a20] text-gray-900 dark:text-gray-100 rounded-3xl max-w-5xl w-full p-5 sm:p-6 shadow-2xl border border-gray-200/90 dark:border-white/10 flex flex-col h-[88vh] max-h-[850px] min-h-[550px] transition-all animate-in fade-in zoom-in-95 duration-200">
                {/* Header */}
                <div className="flex items-center justify-between pb-3.5 border-b border-gray-200/80 dark:border-white/10 shrink-0">
                    <div className="flex items-center gap-2.5">
                        <div className="p-2 rounded-xl bg-purple-500/10 border border-purple-500/20 text-purple-600 dark:text-purple-400 shadow-sm">
                            <Cpu className="w-5 h-5" />
                        </div>
                        <div>
                            <div className="flex items-center gap-2">
                                <h3 className="text-base sm:text-lg font-bold tracking-tight">
                                    {t('token_stats.native_collector', '原生统计管理')}
                                </h3>
                                <span className="px-2 py-0.5 rounded-full text-[11px] font-semibold bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300 border border-purple-300 dark:border-purple-800">
                                    Antigravity 2.26.0
                                </span>
                            </div>
                            <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                                {t(
                                    'token_stats.native_baseline_notice',
                                    '首版限定为 Windows、Antigravity 2.26.0 原生客户端。首次启用时建立时间与记录基线，历史生成不计入，源数据库始终只读访问。'
                                )}
                            </p>
                        </div>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-2 text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 rounded-xl hover:bg-gray-100 dark:hover:bg-white/5 transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Metrics & Control Banner */}
                <div className="mt-4 p-4 rounded-2xl bg-gray-50/80 dark:bg-white/[0.03] border border-gray-200/80 dark:border-white/10 grid grid-cols-1 sm:grid-cols-4 gap-4 items-center shrink-0">
                    {/* Enable Toggle */}
                    <div className="flex items-center justify-between sm:justify-start gap-3">
                        <div>
                            <div className="text-xs font-semibold text-gray-700 dark:text-gray-200">
                                {t('token_stats.native_collector_status', '原生采集状态')}
                            </div>
                            <div className="text-[11px] text-gray-500 dark:text-gray-400">
                                {status?.enabled ? t('token_stats.native_enabled', '已开启') : t('token_stats.native_disabled', '已暂停')}
                            </div>
                        </div>
                        <button
                            onClick={handleToggleEnabled}
                            disabled={togglingEnabled || statusLoading}
                            className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none ${
                                status?.enabled ? 'bg-purple-600' : 'bg-gray-300 dark:bg-gray-700'
                            }`}
                        >
                            <span
                                className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                                    status?.enabled ? 'translate-x-6' : 'translate-x-1'
                                }`}
                            />
                        </button>
                    </div>

                    {/* Stats 1: Total Native Records */}
                    <div className="border-l border-gray-200/60 dark:border-white/10 pl-3">
                        <div className="text-[11px] text-gray-500 dark:text-gray-400">
                            {t('token_stats.native_records_total', '原生记录总数')}
                        </div>
                        <div className="text-base font-bold font-mono text-purple-600 dark:text-purple-400 mt-0.5">
                            {formatNumber(status?.total_native_records || 0)}
                        </div>
                    </div>

                    {/* Stats 2: Unassigned count & tokens */}
                    <div className="border-l border-gray-200/60 dark:border-white/10 pl-3">
                        <div className="text-[11px] text-gray-500 dark:text-gray-400">
                            {t('token_stats.native_unassigned_records', '未归属记录')}
                        </div>
                        <div className="flex items-baseline gap-1.5 mt-0.5">
                            <span className="text-base font-bold font-mono text-amber-600 dark:text-amber-400">
                                {status?.unassigned_records || 0}
                            </span>
                            <span className="text-[11px] text-gray-400 font-mono">
                                ({formatNumber(status?.unassigned_tokens || 0)} Tokens)
                            </span>
                        </div>
                    </div>

                    {/* Action: Scan Now & Last Scanned */}
                    <div className="flex items-center justify-end gap-2.5">
                        <div className="text-right hidden sm:block">
                            <div className="text-[10px] text-gray-400">
                                {t('token_stats.native_baseline_start', '采集起点')}: {formatTime(status?.enabled_since || status?.baseline_established_at || 0)}
                            </div>
                            <div className="text-[10px] text-gray-400 mt-0.5">
                                {t('token_stats.native_last_scan', '最近成功')}: <span className="font-mono text-gray-600 dark:text-gray-300">{formatTime(status?.last_scanned_at || 0)}</span>
                            </div>
                        </div>
                        <button
                            onClick={handleScanNow}
                            disabled={refreshing || !status?.enabled}
                            className="px-3.5 py-2 rounded-xl bg-purple-600 hover:bg-purple-700 disabled:bg-purple-400/50 text-white text-xs font-medium flex items-center gap-1.5 shadow-sm transition-all"
                        >
                            <RefreshCw className={`w-3.5 h-3.5 ${refreshing ? 'animate-spin' : ''}`} />
                            {t('token_stats.native_refresh', '立即扫描')}
                        </button>
                    </div>
                </div>

                {/* Error / Anomaly Notice Banner */}
                {status?.last_error && (
                    <div className="mt-2.5 px-3.5 py-2 rounded-xl bg-amber-50 dark:bg-amber-950/40 border border-amber-200 dark:border-amber-800/60 text-amber-700 dark:text-amber-300 text-xs flex items-center justify-between gap-2 shrink-0">
                        <span className="truncate" title={status.last_error}>
                            {t('token_stats.native_last_error', '采集异常 (已保留上次成功结果)')}: {status.last_error}
                        </span>
                        {status.last_error_at ? (
                            <span className="text-[10px] font-mono opacity-75 shrink-0">
                                {formatTime(status.last_error_at)}
                            </span>
                        ) : null}
                    </div>
                )}

                {/* Filter and Batch Action Bar */}
                <div className="mt-4 flex flex-wrap items-center justify-between gap-3 shrink-0">
                    {/* Search */}
                    <form onSubmit={handleSearchSubmit} className="relative flex-1 min-w-[220px] max-w-md">
                        <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
                        <input
                            type="text"
                            value={searchInput}
                            onChange={e => setSearchInput(e.target.value)}
                            placeholder={t('token_stats.native_search_placeholder', '搜索模型 / 账号 / 会话 ID...')}
                            className="w-full pl-9 pr-3 py-1.5 text-xs rounded-xl bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500/20"
                        />
                    </form>

                    {/* Batch Actions */}
                    {selectedKeys.size > 0 && (
                        <div className="flex items-center gap-2 bg-purple-50 dark:bg-purple-950/40 border border-purple-200 dark:border-purple-800/60 rounded-xl px-3 py-1 animate-in fade-in">
                            <span className="text-xs font-medium text-purple-700 dark:text-purple-300">
                                已选 {selectedKeys.size} 条:
                            </span>
                            <select
                                value={batchAccount}
                                onChange={e => setBatchAccount(e.target.value)}
                                className="text-xs py-1 px-2 rounded-lg bg-white dark:bg-base-200 border border-gray-300 dark:border-base-300 text-gray-900 dark:text-gray-100 focus:outline-none"
                            >
                                <option value="">{t('token_stats.native_select_account_placeholder', '选择归属账号...')}</option>
                                {accounts.map(acc => (
                                    <option key={acc.email} value={acc.email}>
                                        {acc.email}
                                    </option>
                                ))}
                            </select>
                            <button
                                onClick={handleBatchAssign}
                                disabled={!batchAccount || batchOperating}
                                className="px-2.5 py-1 text-xs rounded-lg bg-purple-600 hover:bg-purple-700 disabled:opacity-50 text-white font-medium transition-all"
                            >
                                {t('token_stats.native_batch_assign', '批量归属')}
                            </button>
                            <button
                                onClick={handleBatchUnassign}
                                disabled={batchOperating}
                                className="px-2.5 py-1 text-xs rounded-lg bg-gray-200 dark:bg-white/10 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/30 text-gray-700 dark:text-gray-200 font-medium transition-all"
                            >
                                {t('token_stats.native_batch_unassign', '批量取消')}
                            </button>
                        </div>
                    )}
                </div>

                {/* Table Content Area */}
                <div className="mt-3 flex-1 overflow-auto border border-gray-200/80 dark:border-white/10 rounded-2xl bg-white dark:bg-[#181a20]">
                    <table className="w-full text-left text-xs border-collapse">
                        <thead className="bg-gray-50/90 dark:bg-white/[0.04] border-b border-gray-200/80 dark:border-white/10 sticky top-0 z-10 backdrop-blur-sm">
                            <tr className="text-gray-500 dark:text-gray-400 font-semibold">
                                <th className="p-3 w-10 text-center">
                                    <input
                                        type="checkbox"
                                        checked={
                                            recordsPage.records.length > 0 &&
                                            selectedKeys.size === recordsPage.records.length
                                        }
                                        onChange={toggleSelectAll}
                                        className="rounded border-gray-300 text-purple-600 focus:ring-purple-500"
                                    />
                                </th>
                                <th className="py-3 px-2 w-32">时间</th>
                                <th className="py-3 px-2">模型</th>
                                <th className="py-3 px-2">Token 明细 (输入 / 缓存 / 输出 / 思考)</th>
                                <th className="py-3 px-2 text-right">总 Token</th>
                                <th className="py-3 px-3">账号归属</th>
                                <th className="py-3 px-2 text-right pr-4">操作</th>
                            </tr>
                        </thead>
                        <tbody className="divide-y divide-gray-100 dark:divide-white/[0.05]">
                            {loadingRecords ? (
                                <tr>
                                    <td colSpan={7} className="text-center py-12 text-gray-400">
                                        <RefreshCw className="w-5 h-5 animate-spin mx-auto mb-2 text-purple-500" />
                                        加载中...
                                    </td>
                                </tr>
                            ) : recordsPage.records.length === 0 ? (
                                <tr>
                                    <td colSpan={7} className="text-center py-12 text-gray-400">
                                        <Layers className="w-8 h-8 mx-auto mb-2 opacity-40" />
                                        {t('token_stats.no_data', '暂无数据')}
                                    </td>
                                </tr>
                            ) : (
                                recordsPage.records.map(record => {
                                    const isSelected = selectedKeys.has(record.record_key);
                                    return (
                                        <tr
                                            key={record.record_key}
                                            className={`hover:bg-gray-50/80 dark:hover:bg-white/[0.02] transition-colors ${
                                                isSelected ? 'bg-purple-50/60 dark:bg-purple-950/20' : ''
                                            }`}
                                        >
                                            <td className="p-3 text-center">
                                                <input
                                                    type="checkbox"
                                                    checked={isSelected}
                                                    onChange={() => toggleSelectKey(record.record_key)}
                                                    className="rounded border-gray-300 text-purple-600 focus:ring-purple-500"
                                                />
                                            </td>
                                            <td className="py-2.5 px-2 font-mono text-[11px] text-gray-500 dark:text-gray-400 whitespace-nowrap">
                                                {formatTime(record.timestamp)}
                                            </td>
                                            <td className="py-2.5 px-2">
                                                <div className="font-medium text-gray-900 dark:text-gray-100 font-mono text-xs">
                                                    {record.model_normalized || record.model || 'unknown'}
                                                </div>
                                                <div className="text-[10px] text-gray-400 font-mono truncate max-w-[160px]" title={record.model_raw || record.model}>
                                                    {record.model_raw || record.model}
                                                </div>
                                            </td>
                                            <td className="py-2.5 px-2 font-mono text-[11px]">
                                                <div className="flex flex-wrap items-center gap-2">
                                                    <span
                                                        className="text-blue-600 dark:text-blue-400"
                                                        title={`原生统一输入 = 未缓存输入(${record.uncached_input_tokens ?? record.input_tokens}) + 缓存读取(${record.cached_tokens}) + 缓存写入(${record.cache_write_tokens || 0})`}
                                                    >
                                                        入: {formatNumber(record.input_tokens)}
                                                    </span>
                                                    <span className="text-emerald-600 dark:text-emerald-400" title="缓存读取 Token">
                                                        缓读: {formatNumber(record.cached_tokens)}
                                                    </span>
                                                    {(record.cache_write_tokens || 0) > 0 && (
                                                        <span className="text-teal-600 dark:text-teal-400" title="缓存写入 Token">
                                                            缓写: {formatNumber(record.cache_write_tokens || 0)}
                                                        </span>
                                                    )}
                                                    <span className="text-purple-600 dark:text-purple-400" title="输出 Token (包含思考子项)">
                                                        出: {formatNumber(record.output_tokens)}
                                                    </span>
                                                    {record.reasoning_tokens > 0 && (
                                                        <span className="text-amber-600 dark:text-amber-400" title="思考 Token (属于输出子项，不重复计入总量)">
                                                            思: {formatNumber(record.reasoning_tokens)}
                                                        </span>
                                                    )}
                                                </div>
                                            </td>
                                            <td className="py-2.5 px-2 text-right font-mono font-bold text-gray-900 dark:text-gray-100">
                                                {formatNumber(record.total_tokens)}
                                                <div className="text-[10px] font-normal text-emerald-600 dark:text-emerald-400"
                                                    title={t('token_stats.throughput_hint', '输出 Token ÷ 生成耗时，包含首字等待；无可靠耗时时不估算。')}>
                                                    {formatTokenSpeed(record.tokens_per_second)}
                                                </div>
                                            </td>
                                            <td className="py-2.5 px-3">
                                                {record.account_email && (record.attribution_type || record.attribution_method) !== 'unassigned' ? (
                                                    <div className="flex items-center gap-1.5">
                                                        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium bg-emerald-50 dark:bg-emerald-950/50 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800">
                                                            <UserCheck className="w-3 h-3 text-emerald-500" />
                                                            {record.account_email}
                                                        </span>
                                                        <span className="text-[10px] text-gray-400">
                                                            {(record.attribution_type || record.attribution_method) === 'manual'
                                                                ? t('token_stats.native_attributed_manually', '手动归属')
                                                                : t('token_stats.native_attributed_by_switch', '根据切换记录归属')}
                                                        </span>
                                                    </div>
                                                ) : (
                                                    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium bg-amber-50 dark:bg-amber-950/50 text-amber-700 dark:text-amber-300 border border-amber-200 dark:border-amber-800">
                                                        <UserX className="w-3 h-3 text-amber-500" />
                                                        {t('token_stats.native_unassigned', '未归属')}
                                                    </span>
                                                )}
                                            </td>
                                            <td className="py-2.5 px-2 text-right pr-4">
                                                <select
                                                    value={record.account_email || ''}
                                                    onChange={e => handleAssignRecord(record.record_key, e.target.value)}
                                                    className="text-[11px] py-1 px-2 rounded-lg bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 text-gray-800 dark:text-gray-200 focus:outline-none"
                                                >
                                                    <option value="">-- {t('token_stats.native_unassigned', '未归属')} --</option>
                                                    {accounts.map(acc => (
                                                        <option key={acc.email} value={acc.email}>
                                                            {acc.email}
                                                        </option>
                                                    ))}
                                                </select>
                                            </td>
                                        </tr>
                                    );
                                })
                            )}
                        </tbody>
                    </table>
                </div>

                {/* Pagination Footer */}
                <div className="mt-3 flex items-center justify-between pt-2 border-t border-gray-200/80 dark:border-white/10 shrink-0 text-xs text-gray-500 dark:text-gray-400">
                    <div>
                        共 {recordsPage.total_count} 条记录，第 {currentPage} / {totalPages} 页
                    </div>
                    <div className="flex items-center gap-1.5">
                        <button
                            onClick={handlePrevPage}
                            disabled={page <= 1 || loadingRecords}
                            className="p-1.5 rounded-lg border border-gray-200 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-white/5 disabled:opacity-40 transition-colors"
                        >
                            <ChevronLeft className="w-4 h-4" />
                        </button>
                        <span className="px-2 font-mono font-medium text-gray-700 dark:text-gray-300">
                            {currentPage}
                        </span>
                        <button
                            onClick={handleNextPage}
                            disabled={page >= totalPages || loadingRecords}
                            className="p-1.5 rounded-lg border border-gray-200 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-white/5 disabled:opacity-40 transition-colors"
                        >
                            <ChevronRight className="w-4 h-4" />
                        </button>
                    </div>
                </div>
            </div>
        </div>,
        document.body
    );
};
