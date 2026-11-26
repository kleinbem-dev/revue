// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		interface Error {
			message: string;
			stack?: string;
		}
		interface PageState {
			selectedReviewId?: string;
			isTriggerOpen?: boolean;
		}
	}
}

declare module 'svelte/elements' {
	interface HTMLDialogAttributes {
		closedby?: 'any' | 'closerequest' | 'none';
	}
}

declare module '*.svg' {
	const content: string;
	export default content;
}

export {};
