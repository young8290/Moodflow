// 只跑一遍时间轴收集音效事件（不截图），写 sfx.json
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
(async () => {
  const b = await chromium.launch(); const p = await b.newPage({ viewport: { width: 1920, height: 1080 } });
  await p.goto('http://127.0.0.1:8765/video.html'); await p.evaluate(() => window.ready);
  const r = await p.evaluate(() => { const n = Math.round(window.TOTAL * 30); for (let f = 0; f < n; f++) window.renderAt(f / 30);
    return { total: window.TOTAL, events: [...window.SFX.values()].sort((a, b) => a.t - b.t) }; });
  require('fs').writeFileSync(__dirname + '/sfx.json', JSON.stringify(r, null, 1)); await b.close();
})();
