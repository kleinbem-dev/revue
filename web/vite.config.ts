import { defineWebConfig } from '@kleinbem/web-config/vite';

export default defineWebConfig({
	adapterOptions: {
		pages: 'build',
		assets: 'build',
		fallback: '200.html'
	},
	server: {
		proxy: {
			'/api': 'http://127.0.0.1:8383',
			'/webhook': 'http://127.0.0.1:8383'
		}
	}
});
