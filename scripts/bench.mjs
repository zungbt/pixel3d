// Usage: npm run dev, then [W=..] [H=..] node scripts/bench.mjs [url]
// Runs pixel3d ?bench on the iGPU through WSL's D3D12 Mesa driver (else Mesa falls back to llvmpipe on the CPU).
// playwright-core isn't a dependency: point PLAYWRIGHT_CORE at any install (npx playwright caches one under ~/.npm/_npx).
const { chromium } = await import(process.env.PLAYWRIGHT_CORE || '/home/dung/.npm/_npx/e41f203b7505f1fb/node_modules/playwright-core/index.mjs');
const url = process.argv[2] || 'http://127.0.0.1:5173/?bench';
const browser = await chromium.launch({
  executablePath: process.env.CHROME || process.env.HOME + '/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell',
  args: ['--use-angle=gl', '--ignore-gpu-blocklist', '--enable-gpu', '--no-sandbox'],
  env: { ...process.env, GALLIUM_DRIVER: 'd3d12', MESA_LOADER_DRIVER_OVERRIDE: 'd3d12', LD_LIBRARY_PATH: '/usr/lib/wsl/lib' },
});
const page = await browser.newPage({ viewport: { width: +(process.env.W || 1920), height: +(process.env.H || 1080) } });
page.on('console', (m) => { const t = m.text(); if (t.startsWith('[bench]') && !t.includes('done')) console.error(t); if (m.type() === 'error') console.error('[error]', t); });
page.on('pageerror', (e) => console.error('[pageerror]', e.message));
await page.goto(url);
await page.waitForFunction(() => window.__bench, null, { timeout: 600000, polling: 1000 });
console.log(JSON.stringify(await page.evaluate(() => window.__bench), null, 1));
await browser.close();
