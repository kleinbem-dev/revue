import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		sveltekit({
			adapter: adapter({
				fallback: '200.html'
			}),
			compilerOptions: {
				experimental: {
					async: true
				},
				fragments: 'html'
			},
			dynamicCompileOptions({ filename }) {
				if (!filename.includes('node_modules')) {
					return { runes: true };
				}
			},
			experimental: {
				forkPreloads: true
			}
		})
	],
	server: {
		proxy: {
			'/api': 'http://127.0.0.1:8383',
			'/webhook': 'http://127.0.0.1:8383'
		}
	}
});
