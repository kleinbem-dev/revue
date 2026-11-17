import { defineWebConfig } from '@kleinbem/web-config/vite';

export default defineWebConfig({
	server: {
		proxy: {
			'/api': 'http://127.0.0.1:8383',
			'/webhook': 'http://127.0.0.1:8383'
		}
	}
});
