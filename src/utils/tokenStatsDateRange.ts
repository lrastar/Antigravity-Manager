export type UsageDateRange = { preset: 'today' | '24h' | '7d' | '14d' | '30d' | 'all' }
    | { preset: 'custom'; start: string; end: string };
export type StatsBucket = 'hourly' | 'daily';

export function getStatsRangeBounds(range: UsageDateRange, now = new Date()): { startTime: number; endTime: number; bucket: StatsBucket } {
    let endTime = Math.floor(now.getTime() / 1000) + 1;
    let startTime = 0;
    if (range.preset === 'custom') {
        if (typeof range.start !== 'string' || typeof range.end !== 'string' || !range.start.trim() || !range.end.trim()) {
            throw new Error('Invalid statistics date range');
        }
        startTime = Math.floor(new Date(range.start).getTime() / 1000);
        endTime = Math.floor(new Date(range.end).getTime() / 1000);
    } else if (range.preset === 'today') {
        startTime = Math.floor(new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000);
    } else if (range.preset !== 'all') {
        const days = { '24h': 1, '7d': 7, '14d': 14, '30d': 30 }[range.preset];
        startTime = Math.floor(now.getTime() / 1000) - days * 86400;
    }
    if (!Number.isFinite(startTime) || !Number.isFinite(endTime) || startTime < 0 || endTime <= startTime) {
        throw new Error('Invalid statistics date range');
    }
    return { startTime, endTime, bucket: range.preset !== 'all' && endTime - startTime <= 86401 ? 'hourly' : 'daily' };
}

export function restoreStatsDateRange(saved: string | null): UsageDateRange {
    try {
        const value = JSON.parse(saved || 'null');
        if (value && ['today', '24h', '7d', '14d', '30d', 'all', 'custom'].includes(value.preset)) {
            getStatsRangeBounds(value);
            return value;
        }
    } catch { /* Invalid or old preferences fall back to a supported range. */ }
    return { preset: '7d' };
}
