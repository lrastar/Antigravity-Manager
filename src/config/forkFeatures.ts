// Keep fork-only page choices in one place so upstream pages and APIs stay mergeable.
const disabledPageRoots = new Set(['apikey-fun', 'user-token']);

export function isForkPageEnabled(path?: string): boolean {
    const root = (path || '').replace(/^\/+/, '').split(/[/?#]/, 1)[0];
    return !disabledPageRoots.has(root);
}
