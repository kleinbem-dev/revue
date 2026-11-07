// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		interface Error {
			message: string;
			stack?: string;
		}
		// interface Locals {}
		// interface PageData {}
		interface PageState {
			selectedReviewId?: string;
			isTriggerOpen?: boolean;
		}
		// interface Platform {}
	}
}

declare module '*.svg' {
	const content: string;
	export default content;
}

export {};
