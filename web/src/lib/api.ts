import type { Attachment } from 'svelte/attachments';

const API_KEY_STORAGE_KEY = 'revue-api-key';

export function getStoredApiKey(): string {
	if (typeof localStorage === 'undefined') return '';
	try {
		return localStorage.getItem(API_KEY_STORAGE_KEY)?.trim() ?? '';
	} catch {
		return '';
	}
}

export function setStoredApiKey(key: string): void {
	if (typeof localStorage === 'undefined') return;
	try {
		const trimmed = key.trim();
		if (trimmed) {
			localStorage.setItem(API_KEY_STORAGE_KEY, trimmed);
		} else {
			localStorage.removeItem(API_KEY_STORAGE_KEY);
		}
	} catch {
		// Ignore storage quota / privacy mode errors
	}
}

export function buildApiHeaders(extra?: HeadersInit, explicitKey?: string): Headers {
	const headers = new Headers(extra);
	const key = explicitKey !== undefined ? explicitKey.trim() : getStoredApiKey();
	if (key && !headers.has('X-Revue-Key') && !headers.has('Authorization')) {
		headers.set('X-Revue-Key', key);
	}
	return headers;
}

export function apiFetch(
	input: string,
	init: RequestInit = {},
	fetchFn: typeof fetch = fetch
): Promise<Response> {
	return fetchFn(input, {
		...init,
		headers: buildApiHeaders(init.headers)
	});
}

export function modalDialog(onClose: () => void): Attachment<HTMLDialogElement> {
	return (dialog) => {
		if (!dialog.open) {
			dialog.showModal();
		}
		if ('closedBy' in HTMLDialogElement.prototype) {
			return () => {
				if (dialog.open) dialog.close();
			};
		}
		const handleClick = (event: MouseEvent) => {
			if (event.target !== dialog) return;
			const rect = dialog.getBoundingClientRect();
			const inside =
				rect.top <= event.clientY &&
				event.clientY <= rect.top + rect.height &&
				rect.left <= event.clientX &&
				event.clientX <= rect.left + rect.width;
			if (!inside) {
				dialog.close();
				onClose();
			}
		};
		dialog.addEventListener('click', handleClick);
		return () => {
			dialog.removeEventListener('click', handleClick);
			if (dialog.open) dialog.close();
		};
	};
}
