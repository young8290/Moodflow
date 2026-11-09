// Exercise the packaged WebView and native commands. No mocks or injected IPC.
// CDP is enabled only in the disposable runner process by ci-hub-smoke.ps1.
import assert from 'node:assert/strict'
import { writeFile, mkdir } from 'node:fs/promises'
import path from 'node:path'

assert.equal(process.env.GITHUB_ACTIONS, 'true', 'isolated Windows CI only')
assert.equal(process.platform, 'win32')
const [portText, reportPath] = process.argv.slice(2)
const port = Number(portText)
assert(Number.isInteger(port) && port > 1024 && port < 65536)
assert(path.isAbsolute(reportPath))
const report = { checks: [], failure: null }
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
let socket
let nextId = 0
const pending = new Map()
const exceptions = []

async function target() {
  const start = Date.now()
  while (Date.now() - start < 60000) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(2000) })
      assert(response.ok, `CDP discovery returned HTTP ${response.status}`)
      const targets = await response.json()
      report.targets = targets.slice(0, 12).map((item) => ({ type: item.type, url: item.url?.split(/[?#]/)[0] }))
      const found = targets.find((item) => ['page', 'webview'].includes(item.type) && item.url.includes('/dashboard/'))
      if (found) {
        const url = new URL(found.webSocketDebuggerUrl)
        assert(['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname), 'CDP must remain on loopback')
        assert.equal(Number(url.port), port)
        url.hostname = '127.0.0.1'
        return url.href
      }
    } catch (error) { report.connectionError = String(error.message).slice(0, 300) }
    await sleep(250)
  }
  throw new Error('Packaged Hub did not expose its dashboard WebView')
}
async function connect() {
  socket = new WebSocket(await target())
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', reject, { once: true })
  })
  socket.addEventListener('message', ({ data }) => {
    const message = JSON.parse(String(data))
    if (message.method === 'Runtime.exceptionThrown') exceptions.push(message.params.exceptionDetails.text)
    if (!message.id) return
    const receiver = pending.get(message.id)
    if (!receiver) return
    pending.delete(message.id)
    clearTimeout(receiver.timer)
    if (message.error) receiver.reject(new Error(message.error.message))
    else receiver.resolve(message.result)
  })
  await command('Runtime.enable')
  await command('Page.enable')
}
function command(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++nextId
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)) }, 15000)
    pending.set(id, { resolve, reject, timer })
    socket.send(JSON.stringify({ id, method, params }))
  })
}
async function evaluate(expression) {
  const reply = await command('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text)
  return reply.result.value
}
async function wait(expression, stage, timeout = 20000) {
  const start = Date.now()
  while (Date.now() - start < timeout) {
    try {
      if (await evaluate(`Boolean(${expression})`)) return
    } catch (error) {
      if (!/Cannot find context|Execution context was destroyed|Inspected target navigated/.test(error.message)) throw error
    }
    await sleep(200)
  }
  throw new Error(stage)
}
async function click(selector) {
  await wait(`document.querySelector(${JSON.stringify(selector)}) && !document.querySelector(${JSON.stringify(selector)}).disabled`, `Missing enabled control: ${selector}`)
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`)
}
async function input(selector, value) {
  await wait(`document.querySelector(${JSON.stringify(selector)}) && !document.querySelector(${JSON.stringify(selector)}).disabled`, `Missing editable field: ${selector}`)
  await evaluate(`(() => { const e=document.querySelector(${JSON.stringify(selector)}); e.value=${JSON.stringify(value)}; e.dispatchEvent(new Event('input',{bubbles:true})); })()`)
}
async function page(index, selector) {
  await wait(`document.querySelectorAll('nav.nav > button').length >= 15`, 'Dashboard navigation did not render')
  await evaluate(`document.querySelectorAll('nav.nav > button')[${index}].click()`)
  await wait(`document.querySelector(${JSON.stringify('main.page '+selector)})`, `Tool page did not open: ${selector}`)
}
async function clean() {
  const errors = await evaluate(`Array.from(document.querySelectorAll('main.page [role=alert]')).filter(e=>e.textContent.trim()).map(e=>e.textContent.trim())`)
  assert.deepEqual(errors, [], 'Native tool reported an error')
  assert.deepEqual(exceptions, [], 'Uncaught WebView exception')
}
async function passed(name, screenshot = false) {
  await clean()
  report.checks.push(name)
  if (screenshot) {
    const image = await command('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false })
    await writeFile(path.join(path.dirname(reportPath), `hub-ui-${name}.png`), Buffer.from(image.data, 'base64'))
  }
  console.log(`PASS packaged Hub: ${name}`)
}

try {
  await mkdir(path.dirname(reportPath), { recursive: true })
  await connect()
  await wait(`window.__TAURI_INTERNALS__ && document.querySelector('nav.nav')`, 'Actual Tauri bridge did not initialize')

  await page(7, '.library')
  await click('.library-head button.primary')
  await input('.library .editor input.title', 'Moodflow CI synthetic note')
  await input('.library .editor textarea', 'Synthetic packaged UI/native persistence receipt.')
  await click('.library .editor-toolbar button.primary')
  await wait(`Array.from(document.querySelectorAll('.library .entry strong')).some(e=>e.textContent==='Moodflow CI synthetic note') && !document.querySelector('.library .dirty')`, 'Native note save did not complete')
  await passed('library-save', true)
  await command('Page.reload')
  await page(7, '.library')
  await wait(`Array.from(document.querySelectorAll('.library .entry strong')).some(e=>e.textContent==='Moodflow CI synthetic note')`, 'Saved note did not survive a real WebView reload')
  await evaluate(`Array.from(document.querySelectorAll('.library .entry')).find(e=>e.textContent.includes('Moodflow CI synthetic note')).click()`)
  await wait(`document.querySelector('.library .editor textarea')?.value==='Synthetic packaged UI/native persistence receipt.'`, 'Native note reload changed its body')
  await passed('library-reload')

  await page(8, '.translation')
  await input('.translation textarea', '中国')
  await wait(`/China/i.test(document.querySelector('.translation')?.textContent??'')`, 'Packaged offline CC-CEDICT did not return China', 40000)
  await passed('offline-dictionary', true)

  await page(9, '.vault')
  await wait(`document.querySelectorAll('.vault form.unlock > label input[type=password]').length===2`, 'Fresh native vault was not available')
  const master = 'Synthetic CI master passphrase 47!'
  await input('.vault form.unlock > label:first-of-type input', master)
  await input('.vault form.unlock > label:nth-of-type(2) input', master)
  await click('.vault form.unlock button.primary')
  await wait(`document.querySelector('.vault .toolbar')`, 'Native DPAPI vault creation failed')
  await click('.vault .toolbar button:first-child')
  await input('.vault form.editor > label:first-of-type input', 'CI synthetic credential')
  await input('.vault form.editor input[type=password]', 'Synthetic entry password 42!')
  await click('.vault form.editor button.primary')
  await wait(`Array.from(document.querySelectorAll('.vault aside button')).some(e=>e.textContent.trim()==='CI synthetic credential')`, 'Native encrypted entry save failed')
  await click('.vault header button')
  await wait(`document.querySelector('.vault form.unlock') && !document.querySelector('.vault form.editor')`, 'Vault lock did not remove the editor')
  await input('.vault form.unlock > label:first-of-type input', master)
  await click('.vault form.unlock button.primary')
  await wait(`Array.from(document.querySelectorAll('.vault aside button')).some(e=>e.textContent.trim()==='CI synthetic credential')`, 'Native vault did not decrypt the saved entry')
  await click('.vault header button')
  await passed('vault-native-roundtrip')

  await page(12, '.workspace')
  await click('.workspace [role=tab]:nth-child(2)')
  await click('.workspace aside > button:first-child')
  await input('.workspace article > label:first-of-type input', 'CI synthetic local inbox')
  await click('.workspace article > .actions button:first-child')
  await wait(`document.querySelector('.workspace article h2')?.textContent==='CI synthetic local inbox'`, 'Native local inbox creation failed')
  await input('.workspace article > label textarea', 'Synthetic local note; no external sender or model.')
  await click('.workspace article > .actions:last-of-type button:first-child')
  await wait(`document.querySelector('.workspace .messages')?.textContent.includes('Synthetic local note; no external sender or model.')`, 'Native workspace note did not persist')
  await passed('workspace-local-note', true)

  await page(14, '.tool-page')
  await click('.tool-page .action-tabs button:nth-child(7)')
  await input('.tool-page > label textarea', String.raw`\alpha + \beta`)
  await click('.tool-page > .row button:last-child')
  await wait(`document.querySelector('.tool-page .result textarea')?.value==='α + β'`, 'Offline native LaTeX conversion failed')
  assert.equal(await evaluate(`document.querySelector('.connection input[type=checkbox]').checked`), false, 'Online consent changed without user action')
  await passed('offline-latex-and-consent', true)
  await page(11, '.capture')
  await passed('capture-entry')
  await page(13, '.sync-page')
  await passed('sync-entry')
} catch (error) {
  report.failure = String(error.message ?? error).slice(0, 1500)
  if (socket?.readyState === WebSocket.OPEN) {
    try {
      report.alerts = await evaluate(`Array.from(document.querySelectorAll('main.page [role=alert]')).map(e=>e.textContent.trim().slice(0,300))`)
    } catch { /* Preserve the original error if the WebView has gone away. */ }
  }
  process.exitCode = 1
} finally {
  await writeFile(reportPath, JSON.stringify(report, null, 2))
  for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error('Test finished')) }
  socket?.close()
}
