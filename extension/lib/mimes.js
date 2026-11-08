/* rldyour-clipboard — which representations to keep and which to serve back
 * Copyright (C) 2026 NDDev OpenNetwork
 *
 * Deliberately free of St, Clutter and Meta. These are the decisions worth
 * testing — what gets archived, what gets pasted — and keeping them out of the
 * modules that touch the compositor is what lets them be tested at all,
 * without a running shell.
 */

import {SENSITIVE, PROTOCOL_TARGETS, RANK, MAX_REPRESENTATIONS} from './contract.js';
export {MAX_REPRESENTATIONS};

/** Whether one advertised type marks the whole event as a secret. */
export function isSensitive(mime) {
    return SENSITIVE.includes(mime.trim().toLowerCase());
}

export function anySensitive(offered) {
    return offered.some(isSensitive);
}

export function rank(mime) {
    const lowered = mime.trim().toLowerCase();
    const known = RANK[lowered];
    if (known !== undefined)
        return known;
    if (lowered.startsWith('image/'))
        return 5;
    if (lowered.startsWith('text/'))
        return 34;
    return 100;
}

/**
 * The representations worth archiving, in the order the daemon prefers them.
 *
 * Targets that describe the selection rather than carry it are dropped, and
 * the list is capped so a source advertising dozens of near-duplicates costs a
 * handful of transfers rather than all of them.
 */
export function recordable(offered) {
    const seen = new Set();
    const wanted = [];

    for (const mime of offered) {
        const key = mime.trim().toLowerCase();
        if (key === '' || seen.has(key))
            continue;
        seen.add(key);
        if (PROTOCOL_TARGETS.includes(key))
            continue;
        // A target with no slash and no known name is an X11 atom, not a media
        // type; archiving it would record protocol noise as content.
        if (!key.includes('/') && RANK[key] === undefined)
            continue;
        wanted.push(mime.trim());
    }

    wanted.sort((a, b) => rank(a) - rank(b));
    return wanted.slice(0, MAX_REPRESENTATIONS);
}

/**
 * The representation to restore when the caller does not name one.
 *
 * Only one can be offered back — `MetaSelectionSourceMemory` carries a single
 * mime type, and a custom multi-type source cannot be written in an extension
 * because GJS will not implement a vfunc that takes a callback, which
 * `read_async` does. So the choice has to fail safe.
 *
 * Images and file references restore as themselves; there is no ambiguity.
 * Text restores as plain text even when the entry also holds HTML: plain text
 * pasted into a rich editor is merely unstyled, whereas HTML offered to a
 * terminal or a search field matches nothing and pastes nothing at all.
 */
export function preferredMime(entry) {
    const mimes = entry.mimes ?? [];
    if (mimes.length === 0)
        return null;

    if (entry.kind === 'image' || entry.kind === 'files')
        return mimes[0];

    const plain = mimes.find(mime => {
        const lowered = mime.toLowerCase();
        return lowered === 'text/plain' ||
            lowered === 'text/plain;charset=utf-8' ||
            lowered === 'utf8_string';
    });
    return plain ?? mimes[0];
}
