/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const PRIVATE_SERVER = "http://127.0.0.1:8081";

export default defineConfig({
	plugins: [react()],
	build: {
		outDir: "dist",
		emptyOutDir: true,
	},
	server: {
		port: 8090,
		strictPort: true,
		proxy: {
			"/api/private_server": PRIVATE_SERVER,
		},
	},
	test: {
		environment: "jsdom",
		include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
		globals: true,
	},
});
