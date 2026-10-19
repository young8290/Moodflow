// 逐帧渲染 video.html → moodflow-showcase.mp4（1920×1080，30fps，无声，供后期配乐）
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const { spawn } = require('child_process');
const FPS = 30;
(async () => {
  const b = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1920, height: 1080 } });
  p.on('pageerror', e => console.log('ERR', e.message));
  await p.goto('http://127.0.0.1:8765/video.html');
  await p.evaluate(() => window.ready);
  const total = await p.evaluate(() => window.TOTAL);
  const timeline = await p.evaluate(() => window.TIMELINE);
  require('fs').writeFileSync(__dirname + '/timeline.json', JSON.stringify(timeline, null, 2));
  const n = Math.round(total * FPS);
  const ff = spawn('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'image2pipe', '-framerate', String(FPS), '-c:v', 'mjpeg', '-i', '-',
    '-c:v', 'libx264', '-preset', 'medium', '-crf', '18', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', __dirname + '/video-only.mp4'],
    { stdio: ['pipe', 'inherit', 'inherit'] });
  const t0 = Date.now();
  for (let f = 0; f < n; f++) {
    await p.evaluate(x => window.renderAt(x), f / FPS);
    const buf = await p.screenshot({ type: 'jpeg', quality: 92 });
    if (!ff.stdin.write(buf)) await new Promise(r => ff.stdin.once('drain', r));
    if (f % 300 === 0) console.log(`frame ${f}/${n}  ${((Date.now() - t0) / 1000).toFixed(0)}s`);
  }
  ff.stdin.end();
  const ev = await p.evaluate(() => [...window.SFX.values()].sort((a, b) => a.t - b.t));
  require('fs').writeFileSync(__dirname + '/sfx.json', JSON.stringify({ total, events: ev }, null, 1));
  await new Promise(r => ff.on('close', r));
  await b.close();
  console.log('done', ((Date.now() - t0) / 1000).toFixed(0) + 's');
})();
