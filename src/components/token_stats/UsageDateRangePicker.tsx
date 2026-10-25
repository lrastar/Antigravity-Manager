import { useState } from 'react';
import { CalendarDays } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getStatsRangeBounds, type UsageDateRange } from '../../utils/tokenStatsDateRange';

const options = [
    ['today', '今天'], ['24h', '最近 24 小时'], ['7d', '最近 7 天'], ['14d', '最近 14 天'],
    ['30d', '最近 30 天'], ['all', '全部'], ['custom', '自定义'],
] as const;

function localDateTime(date: Date) {
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}T${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

export function UsageDateRangePicker({ value, onChange }: { value: UsageDateRange; onChange: (value: UsageDateRange) => void }) {
    const { t } = useTranslation();
    const [editing, setEditing] = useState(false);
    const [start, setStart] = useState('');
    const [end, setEnd] = useState('');
    const [invalid, setInvalid] = useState(false);
    const openCustom = () => {
        setStart(value.preset === 'custom' ? value.start : localDateTime(new Date(Date.now() - 7 * 86400000)));
        setEnd(value.preset === 'custom' ? value.end : localDateTime(new Date()));
        setInvalid(false);
        setEditing(true);
    };
    return (
        <div className="relative">
            <div className="flex items-center gap-2 px-3 py-1.5 rounded-xl border border-gray-200 dark:border-base-300 bg-gray-100 dark:bg-base-200 text-xs text-gray-700 dark:text-gray-200">
                <button type="button" onClick={openCustom} aria-label={t('token_stats.range_custom', '自定义')} className="rounded focus-visible:outline focus-visible:outline-2"><CalendarDays className="h-4 w-4" /></button>
                <select aria-label={t('token_stats.date_range', '时间范围')} value={editing ? 'custom' : value.preset} className="bg-transparent outline-none py-1" onChange={event => {
                    const preset = event.target.value as UsageDateRange['preset'];
                    if (preset === 'custom') openCustom();
                    else { setEditing(false); onChange({ preset }); }
                }}>
                    {options.map(([preset, label]) => <option key={preset} value={preset} className="bg-white dark:bg-base-200">{t(`token_stats.range_${preset}`, label)}</option>)}
                </select>
            </div>
            {editing && (
                <form className="fixed left-4 right-4 top-32 sm:absolute sm:left-auto sm:right-0 sm:top-full mt-2 z-50 sm:w-80 space-y-3 rounded-xl border border-gray-200 dark:border-base-300 bg-white dark:bg-base-100 p-4 shadow-lg text-xs" onKeyDown={event => { if (event.key === 'Escape') setEditing(false); }} onSubmit={event => {
                    event.preventDefault();
                    const form = new FormData(event.currentTarget);
                    const next: UsageDateRange = { preset: 'custom', start: String(form.get('start') || ''), end: String(form.get('end') || '') };
                    try { getStatsRangeBounds(next); onChange(next); setEditing(false); setInvalid(false); }
                    catch { setInvalid(true); }
                }}>
                    <label className="block space-y-1"><span>{t('token_stats.range_start', '开始时间')}</span><input required name="start" type="datetime-local" defaultValue={start} className="w-full rounded-lg border border-gray-200 dark:border-base-300 bg-transparent p-2" /></label>
                    <label className="block space-y-1"><span>{t('token_stats.range_end', '结束时间')}</span><input required name="end" type="datetime-local" defaultValue={end} className="w-full rounded-lg border border-gray-200 dark:border-base-300 bg-transparent p-2" /></label>
                    {invalid && <p role="alert" className="text-red-500">{t('token_stats.range_invalid', '开始时间必须早于结束时间')}</p>}
                    <div className="flex justify-end gap-2"><button type="button" onClick={() => setEditing(false)} className="rounded-lg px-3 py-2">{t('common.cancel', '取消')}</button><button type="submit" className="rounded-lg bg-blue-600 px-3 py-2 text-white">{t('token_stats.range_apply', '应用')}</button></div>
                </form>
            )}
        </div>
    );
}
