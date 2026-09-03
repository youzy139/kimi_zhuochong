/*
 * 月兔娘 · Kimi 额度桌面挂件（前端）
 *
 * 移植自开源项目 DeepSeek-Balance-Whale-Widget（MIT License，
 * 原作者 MeteorNOX），保留其核心交互：气泡 SVG 几何、四分之一吸附、
 * 按压 Q 弹、数字滚动、汉堡菜单、加权随机台词；换皮为「月兔娘」
 * （暗夜蓝紫 + 月光银配色），数据源改为 Tauri IPC。
 *
 * 运行环境：
 *  - Tauri v2（withGlobalTauri）：window.__TAURI__ 存在，窗口移动/尺寸
 *    通过 __TAURI__.window API，数据通过 __TAURI__.core.invoke。
 *  - 纯浏览器（无 __TAURI__）：自动进入 mock 模式，内置假数据 +
 *    localStorage 配置，页面可独立打开预览调试。
 */
(function () {
if (window.__kimiRabbitWidget) return
window.__kimiRabbitWidget = true

// ---------- 常量 ----------
var MIN_SCALE = 0.6
var MAX_SCALE = 2.5
var BASE_PX = 320            // scale=1 时挂件（=窗口）边长；角色占其中约 60%
var CLICK_SQ = 9             // 拖拽/点击判定阈值（平方距离）
var REFRESH_MS = 60000       // 自动刷新间隔
var CHANGE_MS = 900
var ANIM_MS = 700            // 数字滚动时长
var BUBBLE_MS = 5000         // 气泡自动收起
var SNAP_ANIM_MS = 200       // 吸附滑动动画
var IMG_URL = 'assets/rabbit.png'
var GIF_URL = 'assets/rua.gif'   // 随机台词动图（可选素材，缺失时降级为文字台词）

// ---------- 环境检测：Tauri or 浏览器 mock ----------
var TAURI = window.__TAURI__ || null
var tauriWin = null
try { tauriWin = (TAURI && TAURI.window) ? TAURI.window.getCurrentWindow() : null } catch (err) {}
var isTauri = !!(TAURI && TAURI.core && tauriWin)

// mock 数据（仅浏览器预览用）：模拟 60 秒刷新间额度缓慢变化
var mockState = { percent: 12.3, used: 12345, seq: 7, calls: 0 }
function mockInvoke(cmd, args) {
  return new Promise(function (resolve) {
    setTimeout(function () {
      if (cmd === 'get_usage') {
        mockState.calls++
        mockState.percent = Math.min(96, mockState.percent + 1.3)
        mockState.used = Math.round(100000 * mockState.percent / 100)
        if (mockState.calls % 3 === 0) mockState.seq++   // 每 3 次刷新模拟新的一轮对话
        var st = mockState.percent > 60 ? 'warn' : 'ok'
        resolve({
          source: 'ledger',
          week: {
            used_tokens: mockState.used,
            total_tokens: 100000,
            percent: Math.round(mockState.percent * 10) / 10,
            reset_at: Date.now() + 3.4 * 86400000
          },
          window5h: { used_tokens: 8000 + mockState.calls * 500, warn_threshold: 50000, status: st },
          fuel_pack: 25.5,
          last_turn: { tokens: 2100, seq: mockState.seq },
          updated_at: Date.now()
        })
        return
      }
      if (cmd === 'get_config') {
        try { resolve(JSON.parse(localStorage.getItem('krw-config') || 'null') || {}) } catch (err) { resolve({}) }
        return
      }
      if (cmd === 'set_config') {
        try {
          var cur = JSON.parse(localStorage.getItem('krw-config') || '{}')
          var patch = (args && args.patch) || {}
          for (var k in patch) cur[k] = patch[k]
          localStorage.setItem('krw-config', JSON.stringify(cur))
        } catch (err) {}
        resolve(null)
        return
      }
      resolve(null)
    }, 60)
  })
}
function invoke(cmd, args) {
  if (isTauri) return TAURI.core.invoke(cmd, args)
  return mockInvoke(cmd, args)
}

// ---------- 样式（暗夜蓝紫 + 月光银） ----------
var C_STROKE = '#4a3f8f'       // 气泡描边深紫蓝
var C_BUBBLE_BG = 'rgba(21,17,46,.92)'   // 气泡暗夜底
var C_TEXT = '#d5dcf0'         // 月光银主文字
var C_SUB = '#9a8fd0'          // 淡紫次文字
var C_OK = '#7dd8a8'
var C_WARN = '#e6c05a'
var C_UNKNOWN = '#8a8fa8'
var C_GOLD = '#eec95f'         // 消耗数字

var css = [
  '.krw-root{position:fixed;left:0;top:0;--krw-scale:1;width:calc(' + BASE_PX + 'px * var(--krw-scale));height:calc(' + BASE_PX + 'px * var(--krw-scale));pointer-events:none;user-select:none;-webkit-user-select:none;z-index:9999;transition:left .18s ease,top .18s ease,transform .3s ease}',
  '.krw-root.krw-left{transform:scaleX(-1)}',
  '.krw-root.krw-dragging{cursor:grabbing;transition:none}',
  '.krw-body{position:absolute;left:0;top:0;width:100%;height:100%;transform-origin:50% 100%;transition:transform .22s cubic-bezier(.34,1.56,.64,1)}',
  // 角色图：原图为带星空背景的正方形，裁成圆形「月亮徽章」构图，右下角
  '.krw-img{position:absolute;right:0;bottom:0;width:60%;height:60%;display:block;border-radius:50%;box-shadow:0 0 24px rgba(154,143,208,.35);pointer-events:none;-webkit-user-drag:none;user-select:none;object-fit:cover}',
  '.krw-bubble{position:absolute;left:0;top:0;width:100%;aspect-ratio:1026/700;pointer-events:none;z-index:1;--krw-u:calc(' + BASE_PX + 'px * var(--krw-scale) / 1026)}',
  '.krw-bubble svg{display:block;width:100%;height:100%;pointer-events:none}',
  '.krw-bubble svg path,.krw-bubble svg ellipse{pointer-events:none;cursor:pointer}',
  '.krw-bubble.krw-bubble-open svg path,.krw-bubble.krw-bubble-open svg ellipse{pointer-events:visiblePainted}',
  '.krw-bubble .krw-bshape,.krw-bubble .krw-b1,.krw-bubble .krw-b2{opacity:0;transform:scale(.7);transform-box:fill-box;transform-origin:50% 50%;transition:opacity .2s ease,transform .2s ease}',
  '.krw-bubble.krw-bubble-open .krw-bshape,.krw-bubble.krw-bubble-open .krw-b1,.krw-bubble.krw-bubble-open .krw-b2{opacity:1;transform:none}',
  '.krw-bubble.krw-bubble-open .krw-b2{transition-delay:0s}',
  '.krw-bubble.krw-bubble-open .krw-b1{transition-delay:.13s}',
  '.krw-bubble.krw-bubble-open .krw-bshape{transition-delay:.26s}',
  '.krw-bubble .krw-bshape{transition-delay:.1s}',
  '.krw-bubble .krw-b1{transition-delay:.2s}',
  '.krw-bubble .krw-b2{transition-delay:.3s}',
  '.krw-text{position:absolute;left:44.25%;top:37%;transform:translate(-50%,-50%);text-align:center;color:' + C_TEXT + ';line-height:1.15;white-space:nowrap;pointer-events:none;opacity:0;transition:opacity .16s ease,transform .3s ease;font-family:"PingFang SC","HarmonyOS Sans SC","Microsoft YaHei","Segoe UI",sans-serif}',
  // 随机台词动图：与文字块同锚点，默认隐藏
  '.krw-gif{position:absolute;left:44.25%;top:37%;transform:translate(-50%,-50%);max-width:calc(var(--krw-u) * 520);max-height:calc(var(--krw-u) * 380);display:none;opacity:0;transition:opacity .2s ease;pointer-events:none;-webkit-user-drag:none;user-select:none;object-fit:contain;border-radius:calc(var(--krw-u) * 24)}',
  '.krw-bubble.krw-bubble-open .krw-gif{opacity:1}',
  '.krw-root.krw-left .krw-gif{transform:translate(-50%,-50%) scaleX(-1)}',
  '.krw-bubble.krw-bubble-open .krw-text{opacity:1;transition:opacity .16s ease .36s,transform .3s ease}',
  '.krw-root.krw-left .krw-text{transform:translate(-50%,-50%) scaleX(-1)}',
  '.krw-label{font-size:calc(var(--krw-u) * 46);font-weight:600;letter-spacing:.08em;color:' + C_SUB + '}',
  '.krw-amount{font-size:calc(var(--krw-u) * 96);font-weight:800;line-height:1.05;color:' + C_TEXT + ';text-shadow:0 0 8px rgba(213,220,240,.25);font-variant-numeric:tabular-nums}',
  '.krw-period{font-size:calc(var(--krw-u) * 76);font-weight:800;line-height:1.05}',
  '.krw-hint{font-size:calc(var(--krw-u) * 40);color:' + C_SUB + ';letter-spacing:.04em;margin-top:calc(var(--krw-u) * 8);min-height:calc(var(--krw-u) * 46);line-height:1.2}',
  '.krw-wrap{white-space:normal;max-width:calc(var(--krw-u) * 540);line-height:1.25}',
  // 额度进度条（画在气泡文字区里）
  '.krw-bar{width:calc(var(--krw-u) * 420);height:calc(var(--krw-u) * 18);margin:calc(var(--krw-u) * 10) auto 0;border-radius:calc(var(--krw-u) * 9);background:rgba(154,143,208,.22);overflow:hidden}',
  '.krw-bar-fill{height:100%;width:0%;border-radius:inherit;background:linear-gradient(90deg,#9a8fd0,#d5dcf0);transition:width .6s cubic-bezier(.34,1.2,.64,1)}',
  '.krw-bar-fill.krw-bar-low{background:linear-gradient(90deg,#c05a6e,#e6c05a)}',
  // 汉堡按钮（悬停角色时出现）
  '.krw-menu-btn{position:absolute;top:calc(40% + 4px);right:4px;width:26px;height:26px;border:none;border-radius:6px;background:rgba(61,52,120,.85);cursor:pointer;pointer-events:auto;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:4px;padding:0;z-index:2;opacity:0;transition:opacity .15s ease}',
  '.krw-menu-btn.krw-menu-btn-visible{opacity:1}',
  '.krw-menu-btn span{display:block;width:14px;height:2px;background:' + C_TEXT + ';border-radius:1px}',
  '.krw-menu-btn:hover{background:#3d3478}',
  // 菜单：深色半透明
  '.krw-menu{position:fixed;min-width:210px;max-width:260px;background:rgba(21,17,46,.94);border:1px solid rgba(74,63,143,.55);border-radius:10px;padding:10px 12px;opacity:0;transform:scale(.92) translateY(4px);transform-origin:bottom right;transition:opacity .18s ease,transform .2s cubic-bezier(.34,1.56,.64,1);pointer-events:none;z-index:10000;box-shadow:0 6px 20px rgba(0,0,0,.45);color-scheme:dark;max-height:calc(100% - 10px);overflow-y:auto;box-sizing:border-box}',
  '.krw-menu.krw-menu-open{opacity:1;transform:scale(1) translateY(0);pointer-events:auto}',
  '.krw-menu-row{display:flex;align-items:center;gap:8px;margin:5px 0;color:' + C_TEXT + ';font-size:12px;white-space:nowrap;font-family:inherit}',
  '.krw-range{flex:1;min-width:0;accent-color:#9a8fd0}',
  '.krw-number{width:64px;border:1px solid rgba(154,143,208,.4);border-radius:6px;padding:2px 4px;font-size:12px;color:' + C_TEXT + ';background:rgba(154,143,208,.12);box-sizing:border-box}',
  '.krw-select{flex:1;border:1px solid rgba(154,143,208,.4);border-radius:6px;background:rgba(154,143,208,.12);color:' + C_TEXT + ';font-size:12px;padding:3px 0;cursor:pointer}',
  // 下拉展开列表不受 color-scheme:dark 控制，需显式指定深色底浅色字，否则选中项白底白字看不见
  '.krw-select option{background:#241f4a;color:' + C_TEXT + '}',
  '.krw-check{width:16px;height:16px;accent-color:#9a8fd0;cursor:pointer;flex:0 0 auto}',
  '.krw-menu-sep{height:1px;background:rgba(154,143,208,.25);margin:6px 0}',
  '.krw-volpct{width:40px;text-align:right;color:' + C_SUB + ';font-size:12px}',
  '.krw-fuel{color:' + C_GOLD + ';font-size:12px}'
].join('\n')

var styleEl = document.createElement('style')
styleEl.textContent = css
document.head.appendChild(styleEl)

// ---------- DOM ----------
var root = document.createElement('div')
root.className = 'krw-root'

var body = document.createElement('div')
body.className = 'krw-body'

var img = document.createElement('img')
img.className = 'krw-img'
img.src = IMG_URL
img.alt = '月兔娘'
img.draggable = false

// 气泡 SVG：沿用鲸鱼项目的几何（viewBox 1026×700，大椭圆+尾巴+两小气泡），
// 仅换色为暗夜蓝紫底 + 深紫蓝描边
var bubbleBox = document.createElement('div')
bubbleBox.className = 'krw-bubble'
bubbleBox.innerHTML = '<svg viewBox="0 0 1026 700" preserveAspectRatio="xMidYMid meet" xmlns="http://www.w3.org/2000/svg">' +
  '<path class="krw-bshape" fill="' + C_BUBBLE_BG + '" stroke="' + C_STROKE + '" stroke-width="18" stroke-linejoin="round" stroke-linecap="round" d="M 827 248 A 373 232 0 1 0 81 246 A 373 232 0 0 0 301 465 A 57 32 10 0 0 413 484 A 373 232 0 0 0 827 248 Z"/>' +
  '<ellipse class="krw-b1" cx="352" cy="561" rx="37.5" ry="26" fill="' + C_BUBBLE_BG + '" stroke="' + C_STROKE + '" stroke-width="18"/>' +
  '<ellipse class="krw-b2" cx="442" cy="646" rx="24.5" ry="18" fill="' + C_BUBBLE_BG + '" stroke="' + C_STROKE + '" stroke-width="18"/>' +
  '</svg>'

var textBox = document.createElement('div')
textBox.className = 'krw-text'
var labelEl = document.createElement('div')
labelEl.className = 'krw-label'
labelEl.textContent = 'Kimi 本周额度'
var amountEl = document.createElement('div')
amountEl.className = 'krw-amount'
var barEl = document.createElement('div')
barEl.className = 'krw-bar'
var barFillEl = document.createElement('div')
barFillEl.className = 'krw-bar-fill'
barEl.appendChild(barFillEl)
var hintEl = document.createElement('div')
hintEl.className = 'krw-hint'
var statusEl = document.createElement('div')
statusEl.className = 'krw-hint'
textBox.appendChild(labelEl)
textBox.appendChild(amountEl)
textBox.appendChild(barEl)
textBox.appendChild(hintEl)
textBox.appendChild(statusEl)
// 随机台词动图（加载失败时 gifFailed=true，台词组降级为文字）
var gifEl = document.createElement('img')
gifEl.className = 'krw-gif'
gifEl.src = GIF_URL
gifEl.alt = ''
gifEl.draggable = false
var gifFailed = false
gifEl.addEventListener('error', function () { gifFailed = true })
bubbleBox.appendChild(gifEl)
bubbleBox.appendChild(textBox)

var menuBtn = document.createElement('button')
menuBtn.type = 'button'
menuBtn.className = 'krw-menu-btn'
menuBtn.title = '菜单'
menuBtn.innerHTML = '<span></span><span></span><span></span>'
menuBtn.addEventListener('click', function (e) { e.stopPropagation(); toggleMenu() })

body.appendChild(img)
body.appendChild(bubbleBox)
root.appendChild(body)
root.appendChild(menuBtn)
document.body.appendChild(root)

// ---------- 菜单 ----------
var menuBox = document.createElement('div')
menuBox.className = 'krw-menu'
function menuLabel(text) { var s = document.createElement('span'); s.textContent = text; return s }
function menuRow() { var r = document.createElement('div'); r.className = 'krw-menu-row'; return r }
function menuOpt(value, label) { var o = document.createElement('option'); o.value = value; o.textContent = label; return o }

// 大小滑块
var scaleInput = document.createElement('input')
scaleInput.type = 'range'
scaleInput.min = String(MIN_SCALE)
scaleInput.max = String(MAX_SCALE)
scaleInput.step = '0.1'
scaleInput.className = 'krw-range'
scaleInput.value = '1'
var scaleNumber = document.createElement('input')
scaleNumber.type = 'number'
scaleNumber.min = '1'
scaleNumber.max = '20'
scaleNumber.step = '1'
scaleNumber.className = 'krw-number'
scaleNumber.style.width = '44px'
scaleNumber.value = '10'
scaleInput.addEventListener('input', function () { setScale(scaleInput.value) })
scaleNumber.addEventListener('change', function () {
  var v = Math.round(Number(scaleNumber.value))
  setScale(MIN_SCALE + Math.max(0, Math.min(20, v) - 1) * (MAX_SCALE - MIN_SCALE) / 19)
})
// 音效开关 + 音量（无 mp3 素材时静默降级）
var soundToggle = document.createElement('input')
soundToggle.type = 'checkbox'
soundToggle.className = 'krw-check'
soundToggle.checked = true
soundToggle.title = '按压音效开关'
soundToggle.addEventListener('change', function () { setSoundOn(soundToggle.checked) })
var volInput = document.createElement('input')
volInput.type = 'range'
volInput.min = '0'
volInput.max = '1'
volInput.step = '0.05'
volInput.className = 'krw-range'
volInput.value = '0.9'
var volPct = document.createElement('span')
volPct.className = 'krw-volpct'
volPct.textContent = '90%'
volInput.addEventListener('input', function () { setVol(volInput.value) })
// 音效套装选择（小黄鸭 / 叮叮咚咚）
var soundSetSelect = document.createElement('select')
soundSetSelect.className = 'krw-select'
soundSetSelect.appendChild(menuOpt('duck', '小黄鸭'))
soundSetSelect.appendChild(menuOpt('fx1', '叮叮咚咚'))
soundSetSelect.addEventListener('change', function () { setSoundSet(soundSetSelect.value) })
// 气泡 / 每轮消耗开关
var bubbleToggle = document.createElement('input')
bubbleToggle.type = 'checkbox'
bubbleToggle.className = 'krw-check'
bubbleToggle.checked = true
bubbleToggle.title = '开启/关闭台词气泡'
bubbleToggle.addEventListener('change', function () { setBubbleOn(bubbleToggle.checked) })
var turnCostToggle = document.createElement('input')
turnCostToggle.type = 'checkbox'
turnCostToggle.className = 'krw-check'
turnCostToggle.checked = true
turnCostToggle.title = '每轮对话结束后显示本轮 token 消耗'
turnCostToggle.addEventListener('change', function () { setTurnCostOn(turnCostToggle.checked) })
// 每轮消耗泡泡自动关闭秒数（0 = 不自动关闭，点击泡泡手动关）
var turnCostCloseInput = document.createElement('input')
turnCostCloseInput.type = 'number'
turnCostCloseInput.min = '0'
turnCostCloseInput.step = '1'
turnCostCloseInput.className = 'krw-number'
turnCostCloseInput.style.width = '44px'
turnCostCloseInput.value = '5'
turnCostCloseInput.title = '自动关闭秒数，填 0 表示不自动关闭'
turnCostCloseInput.addEventListener('change', function () { setTurnCostClose(turnCostCloseInput.value) })
// 数据源
var sourceSelect = document.createElement('select')
sourceSelect.className = 'krw-select'
sourceSelect.appendChild(menuOpt('auto', '自动（CLI 失败降级记账）'))
sourceSelect.appendChild(menuOpt('ledger', '本地记账'))
sourceSelect.appendChild(menuOpt('cli', 'CLI /usage 解析'))
sourceSelect.addEventListener('change', function () { setDataSource(sourceSelect.value) })
// 周额度上限 / 5h 告警阈值
var quotaInput = document.createElement('input')
quotaInput.type = 'number'
quotaInput.min = '0'
quotaInput.step = '1000'
quotaInput.className = 'krw-number'
quotaInput.title = '每周额度上限（token 数），0 表示未设置'
quotaInput.addEventListener('change', function () { setWeeklyQuota(quotaInput.value) })
var warnInput = document.createElement('input')
warnInput.type = 'number'
warnInput.min = '0'
warnInput.step = '1000'
warnInput.className = 'krw-number'
warnInput.title = '5 小时滚动窗口的告警阈值（token 数）'
warnInput.addEventListener('change', function () { setWarnTokens(warnInput.value) })
// 加油包（fuel_pack 为 null 时整行隐藏）
var fuelRow = menuRow()
var fuelValue = document.createElement('span')
fuelValue.className = 'krw-fuel'
fuelRow.appendChild(menuLabel('加油包'))
fuelRow.appendChild(fuelValue)
fuelRow.style.display = 'none'

var r1 = menuRow(); r1.appendChild(menuLabel('大小')); r1.appendChild(scaleInput); r1.appendChild(scaleNumber)
var r2 = menuRow(); r2.appendChild(menuLabel('音效')); r2.appendChild(soundToggle); r2.appendChild(soundSetSelect); r2.appendChild(volInput); r2.appendChild(volPct)
var r3 = menuRow(); r3.appendChild(menuLabel('气泡')); r3.appendChild(bubbleToggle); r3.appendChild(menuLabel('每轮消耗')); r3.appendChild(turnCostToggle); r3.appendChild(turnCostCloseInput); r3.appendChild(menuLabel('秒'))
var r4 = menuRow(); r4.appendChild(menuLabel('数据源')); r4.appendChild(sourceSelect)
var r5 = menuRow(); r5.appendChild(menuLabel('周额度上限')); r5.appendChild(quotaInput)
var r6 = menuRow(); r6.appendChild(menuLabel('5h 告警阈值')); r6.appendChild(warnInput)
var sep = document.createElement('div'); sep.className = 'krw-menu-sep'
// 退出行：无边框窗口没有标题栏关闭按钮，菜单里给一个显式出口（托盘也可退出）
var quitBtn = document.createElement('button')
quitBtn.type = 'button'
quitBtn.className = 'krw-sound'
quitBtn.textContent = '退出挂件'
quitBtn.title = '关闭月兔娘（也可右键托盘图标退出）'
quitBtn.addEventListener('click', function () {
  if (isTauri) {
    try { window.__TAURI__.window.getCurrentWindow().close() } catch (err) {}
  } else {
    document.body.innerHTML = '<div style="color:#9a8fd0;font:14px sans-serif;padding:20px">mock 模式：请直接关掉这个标签页</div>'
  }
})
var r7 = menuRow(); r7.appendChild(quitBtn)
menuBox.appendChild(r1)
menuBox.appendChild(r2)
menuBox.appendChild(r3)
menuBox.appendChild(r4)
menuBox.appendChild(r5)
menuBox.appendChild(r6)
menuBox.appendChild(sep)
menuBox.appendChild(fuelRow)
menuBox.appendChild(r7)
document.body.appendChild(menuBox)

// ---------- 状态 ----------
var state = {
  scale: 1,
  left: 0, top: 0,          // 屏幕（mock 模式下为页面）CSS px 坐标
  h: 'right', v: 'bottom',  // 吸附边
  usage: null,              // 最近一次 get_usage 结果
  status: 'loading',
  message: ''
}
var busy = false
var drag = null
var shown = null            // 当前气泡上显示的剩余百分比数字
var animId = null
var bubbleShown = false
var bubbleTimer = null
var bubbleRandomActive = false
var bubbleRandomLines = null
var costBubbleActive = false
var costBubbleTimer = null
var lastTurnSeq = -1        // -1 = 尚未对齐
var soundOn = true
var soundVol = 0.9
var bubbleOn = true
var turnCostOn = true
var turnCostCloseMs = 5000   // 每轮消耗泡泡自动关闭毫秒数，0 = 不自动关闭
var menuOpen = false
var winAnimId = null        // Tauri 窗口吸附动画

// ---------- 工具 ----------
function clamp(v, lo, hi) { return v < lo ? lo : (v > hi ? hi : v) }
function pickOne(arr) { return arr[Math.floor(Math.random() * arr.length)] }
function fmtInt(n) {
  var v = Number(n)
  if (!isFinite(v)) return '--'
  return Math.round(v).toLocaleString('en-US')
}
// 屏幕可用区域（吸附判定用）；mock 模式退化为浏览器视口
function screenRect() {
  if (!isTauri) {
    return { x: 0, y: 0, w: window.innerWidth || 1280, h: window.innerHeight || 800 }
  }
  var s = window.screen
  return {
    x: (typeof s.availLeft === 'number') ? s.availLeft : 0,
    y: (typeof s.availTop === 'number') ? s.availTop : 0,
    w: s.availWidth || s.width || 1920,
    h: s.availHeight || s.height || 1080
  }
}
function rootSize() { return BASE_PX * state.scale }

// ---------- 位置抽象：mock = 挪 DOM，Tauri = 挪 OS 窗口 ----------
function express() {
  if (isTauri) {
    root.style.left = '0px'
    root.style.top = '0px'
  } else {
    root.style.left = state.left + 'px'
    root.style.top = state.top + 'px'
  }
  root.classList.toggle('krw-left', state.h === 'left')
}
function tauriSetPosition(x, y) {
  state.left = x
  state.top = y
  try {
    var p = new TAURI.dpi.LogicalPosition(Math.round(x), Math.round(y))
    var r = tauriWin.setPosition(p)
    if (r && typeof r.catch === 'function') r.catch(function () {})
  } catch (err) {}
}
function tauriSetSize(px) {
  try {
    var s = new TAURI.dpi.LogicalSize(Math.round(px), Math.round(px))
    var r = tauriWin.setSize(s)
    if (r && typeof r.catch === 'function') r.catch(function () {})
  } catch (err) {}
}
// 把挂件放到 (x, y)；animate=true 时滑动过去（吸附动画）
function placeWidget(x, y, animate) {
  var size = rootSize()
  var sr = screenRect()
  x = clamp(x, sr.x, sr.x + Math.max(0, sr.w - size))
  y = clamp(y, sr.y, sr.y + Math.max(0, sr.h - size))
  if (!isTauri) {
    state.left = x
    state.top = y
    root.style.transition = animate ? '' : 'none'
    express()
    if (!animate) {
      requestAnimationFrame(function () { root.style.transition = '' })
    }
    return
  }
  if (winAnimId) { cancelAnimationFrame(winAnimId); winAnimId = null }
  express()   // Tauri 模式下翻转 class 也要在这里同步（DOM 位置恒为 0,0）
  if (!animate) { tauriSetPosition(x, y); return }
  var fx = state.left, fy = state.top
  var startT = null
  function step(ts) {
    if (startT === null) startT = ts
    var t = Math.min(1, (ts - startT) / SNAP_ANIM_MS)
    var e = 1 - Math.pow(1 - t, 3)
    tauriSetPosition(fx + (x - fx) * e, fy + (y - fy) * e)
    winAnimId = (t < 1) ? requestAnimationFrame(step) : null
  }
  winAnimId = requestAnimationFrame(step)
}
// 四边四分之一吸附：中心点落在左/右 1/4 → 吸附对应边并翻转；上/下同理
function snapTarget(left, top, size) {
  var sr = screenRect()
  var cx = left + size / 2 - sr.x
  var cy = top + size / 2 - sr.y
  var x = left, y = top, h = null, v = null
  if (cx < sr.w / 4) { h = 'left'; x = sr.x }
  else if (cx > sr.w * 3 / 4) { h = 'right'; x = sr.x + sr.w - size }
  if (cy < sr.h / 4) { v = 'top'; y = sr.y }
  else if (cy > sr.h * 3 / 4) { v = 'bottom'; y = sr.y + sr.h - size }
  return { x: x, y: y, h: h, v: v }
}
function persistPos() {
  saveConfig({ pos: { x: Math.round(state.left), y: Math.round(state.top) } })
}

// ---------- 配置 ----------
function saveConfig(patch) {
  try {
    var r = invoke('set_config', { patch: patch })
    if (r && typeof r.catch === 'function') r.catch(function () {})
  } catch (err) {}
}
function applyConfig(d) {
  if (!d || typeof d !== 'object') return
  if (typeof d.scale === 'number' && d.scale >= MIN_SCALE - 0.1 && d.scale <= MAX_SCALE + 0.1) {
    setScale(d.scale, true)
  }
  if (typeof d.volume === 'number') setVol(d.volume, true)
  if (typeof d.sound_on === 'boolean') setSoundOn(d.sound_on, true)
  if (typeof d.bubble_on === 'boolean') setBubbleOn(d.bubble_on, true)
  if (typeof d.turn_cost_on === 'boolean') setTurnCostOn(d.turn_cost_on, true)
  if (typeof d.turn_cost_close_ms === 'number') setTurnCostClose(d.turn_cost_close_ms / 1000, true)
  if (typeof d.sound_set === 'string') setSoundSet(d.sound_set, true)
  if (typeof d.data_source === 'string' && /^(auto|ledger|cli)$/.test(d.data_source)) {
    sourceSelect.value = d.data_source
  }
  if (typeof d.weekly_quota_tokens === 'number') quotaInput.value = d.weekly_quota_tokens > 0 ? String(d.weekly_quota_tokens) : ''
  if (typeof d.window5h_warn_tokens === 'number') warnInput.value = d.window5h_warn_tokens > 0 ? String(d.window5h_warn_tokens) : ''
  // 位置恢复：pos 为空则默认右下角吸附
  if (d.pos && typeof d.pos.x === 'number' && typeof d.pos.y === 'number') {
    // 恢复时不重新吸附，保持记忆位置；若记忆位置本身在某 1/4 贴边区，
    // 则恢复对应的吸附态与左镜像（snapTarget 只用来推导 h/v）
    var t = snapTarget(d.pos.x, d.pos.y, rootSize())
    state.h = t.h
    state.v = t.v
    placeWidget(d.pos.x, d.pos.y, false)
  } else {
    state.h = 'right'; state.v = 'bottom'
    var sr = screenRect()
    placeWidget(sr.x + sr.w - rootSize(), sr.y + sr.h - rootSize(), false)
  }
}

// ---------- 菜单项 setter（即改即存） ----------
function scaleToDisplay(s) { return Math.round((s - MIN_SCALE) / ((MAX_SCALE - MIN_SCALE) / 19)) + 1 }
function setScale(v, silent) {
  var next = Math.round(clamp(Number(v) || 1, MIN_SCALE, MAX_SCALE) * 10) / 10
  var sr = screenRect()
  // 固定点：角色所在角（未翻转右下 / 翻转左下），缩放时角色始终贴角
  var size0 = rootSize()
  var fx = state.h === 'left' ? state.left : state.left + size0
  var fy = state.top + size0
  state.scale = next
  root.style.setProperty('--krw-scale', String(next))
  scaleInput.value = String(next)
  scaleNumber.value = String(scaleToDisplay(next))
  if (!silent) saveConfig({ scale: next })
  var size1 = rootSize()
  if (isTauri) tauriSetSize(size1)
  var nx = state.h === 'left' ? fx : fx - size1
  var ny = fy - size1
  placeWidget(clamp(nx, sr.x, sr.x + Math.max(0, sr.w - size1)), clamp(ny, sr.y, sr.y + Math.max(0, sr.h - size1)), false)
}
function setVol(v, silent) {
  var next = Math.round(clamp(Number(v) || 0, 0, 1) * 100) / 100
  soundVol = next
  volInput.value = String(next)
  volPct.textContent = Math.round(next * 100) + '%'
  try {
    if (pressAudio) pressAudio.volume = next
    if (releaseAudio) releaseAudio.volume = next
  } catch (err) {}
  if (!silent) saveConfig({ volume: next })
}
function setSoundOn(v, silent) {
  soundOn = !!v
  soundToggle.checked = soundOn
  if (!silent) saveConfig({ sound_on: soundOn })
}
function setBubbleOn(v, silent) {
  bubbleOn = !!v
  bubbleToggle.checked = bubbleOn
  if (!silent) saveConfig({ bubble_on: bubbleOn })
  if (!bubbleOn) hideCostBubble()
}
function setTurnCostOn(v, silent) {
  turnCostOn = !!v
  turnCostToggle.checked = turnCostOn
  turnCostCloseInput.disabled = !turnCostOn
  if (!silent) saveConfig({ turn_cost_on: turnCostOn })
  if (!turnCostOn) hideCostBubble()
}
function setTurnCostClose(v, silent) {
  // 不随开关提前返回：配置恢复时（silent）也要写入秒数，输入框靠 disabled 防误改
  var n = Math.max(0, Math.round(Number(v) || 0))
  turnCostCloseMs = n * 1000
  turnCostCloseInput.value = String(n)
  if (!silent) saveConfig({ turn_cost_close_ms: turnCostCloseMs })
}
function setDataSource(v) {
  var sv = /^(auto|ledger|cli)$/.test(v) ? v : 'auto'
  sourceSelect.value = sv
  saveConfig({ data_source: sv })
  refresh(false)
}
function setWeeklyQuota(v) {
  var n = Math.max(0, Math.round(Number(v) || 0))
  quotaInput.value = n > 0 ? String(n) : ''
  saveConfig({ weekly_quota_tokens: n > 0 ? n : null })
  refresh(false)
}
function setWarnTokens(v) {
  var n = Math.max(0, Math.round(Number(v) || 0))
  warnInput.value = n > 0 ? String(n) : ''
  saveConfig({ window5h_warn_tokens: n > 0 ? n : null })
  refresh(false)
}

// ---------- 音效（assets/press.mp3 / release.mp3 为原项目小黄鸭音效，加载失败静默） ----------
var SQUISH = 'scaleY(0.88) scaleX(1.05)'
var pressAudio = null
var releaseAudio = null
var soundDead = false
// 音效套装：duck=小黄鸭（Ya1/Ya2），fx1=叮叮咚咚（D1/D2），均来自原项目素材
var SOUND_SETS = {
  duck: { press: 'assets/Ya1.mp3', release: 'assets/Ya2.mp3' },
  fx1: { press: 'assets/D1.mp3', release: 'assets/D2.mp3' }
}
var soundSet = 'duck'
function setupSound() {
  try {
    var set = SOUND_SETS[soundSet] || SOUND_SETS.duck
    pressAudio = new Audio(set.press)
    releaseAudio = new Audio(set.release)
    pressAudio.preload = 'auto'
    releaseAudio.preload = 'auto'
    pressAudio.volume = soundVol
    releaseAudio.volume = soundVol
    pressAudio.addEventListener('error', function () { soundDead = true })
    releaseAudio.addEventListener('error', function () { soundDead = true })
  } catch (err) { soundDead = true }
}
function playSound(a) {
  if (!a || !soundOn || soundDead) return
  try {
    a.currentTime = 0
    var p = a.play()
    if (p && typeof p.catch === 'function') p.catch(function () {})
  } catch (err) {}
}
// 按压/松手时序（对齐原项目）：短按 → 松手音与按压音末尾重叠 100ms；
// 长按（松手时按压音已播完）→ 松手时立即播松手音。避免同文件抢断叠音
var pressing = false
var pressEnded = false
var releasePlayed = false
var releaseTimer = null
function pressDown() {
  body.style.transform = SQUISH
  pressing = true
  if (soundDead || !soundOn || !pressAudio) return
  try {
    if (releaseTimer) { clearTimeout(releaseTimer); releaseTimer = null }
    if (releaseAudio) { releaseAudio.pause(); releaseAudio.currentTime = 0 }
    pressEnded = false
    releasePlayed = false
    pressAudio.onended = function () {
      pressEnded = true
      // 时长未知时的兜底：按压音播完且已松手 → 补播松手音
      if (!pressing && !releasePlayed) playRelease()
    }
    playSound(pressAudio)
  } catch (err) {}
}
function playRelease() {
  if (releasePlayed) return
  releasePlayed = true
  playSound(releaseAudio)
}
function pressUp() {
  body.style.transform = 'scaleY(1) scaleX(1)'
  pressing = false
  if (soundDead || !soundOn || !pressAudio) return
  if (pressEnded) { playRelease(); return }
  // 按压音还没播完：让松手音在按压音最后 100ms 切入
  var durKnown = false
  var remainMs = 0
  try {
    var dur = pressAudio.duration
    if (isFinite(dur) && dur > 0) {
      durKnown = true
      remainMs = (dur - pressAudio.currentTime) * 1000
    }
  } catch (err) {}
  if (durKnown) {
    releaseTimer = setTimeout(function () {
      releaseTimer = null
      playRelease()
    }, Math.max(0, remainMs - 100))
  }
  // 时长未知 → 靠 pressAudio.onended 兜底
}
function setSoundSet(v, silent) {
  soundSet = v === 'fx1' ? 'fx1' : 'duck'
  soundSetSelect.value = soundSet
  setupSound()
  if (!silent) saveConfig({ sound_set: soundSet })
}

// ---------- 台词（月兔娘人设：住月球暗面、安静、爱读书、熬夜陪写代码） ----------
var BUBBLE_STYLE_CLASS = { A: 'krw-label', B: 'krw-amount', P: 'krw-period', C: 'krw-hint' }
function singleCenter(style, text, color, wrap) { return [null, { t: text, s: style, c: color || '', w: !!wrap }, null] }
// 剩余百分比（无上限数据时返回 null）
function remainPercent() {
  var u = state.usage
  if (!u || !u.week) return null
  if (typeof u.week.percent !== 'number' || !isFinite(u.week.percent)) return null
  return clamp(100 - u.week.percent, 0, 100)
}
function countdownText() {
  var u = state.usage
  var ra = u && u.week ? u.week.reset_at : null
  if (typeof ra !== 'number' || !isFinite(ra)) return '刷新时间未知'
  var ms = ra - Date.now()
  if (ms <= 0) return '即将刷新'
  var hours = ms / 3600000
  if (hours < 24) return Math.max(1, Math.ceil(hours)) + ' 小时后刷新'
  return Math.ceil(hours / 24) + ' 天后刷新'
}
function window5hText() {
  var u = state.usage
  var w = u && u.window5h
  if (!w) return { t: '5 小时窗口：未知', c: C_UNKNOWN }
  var map = { ok: '正常', warn: '接近限流', unknown: '未知' }
  var cmap = { ok: C_OK, warn: C_WARN, unknown: C_UNKNOWN }
  var st = map[w.status] ? w.status : 'unknown'
  var t = '5 小时窗口：' + map[st]
  if (typeof w.used_tokens === 'number' && typeof w.warn_threshold === 'number') {
    t += '（' + fmtInt(w.used_tokens) + ' / ' + fmtInt(w.warn_threshold) + '）'
  }
  return { t: t, c: cmap[st] }
}
function buildStatusGroup() {
  var rp = remainPercent()
  var w5 = window5hText()
  if (rp === null) {
    var used = state.usage && state.usage.week ? state.usage.week.used_tokens : null
    return [
      { t: '本周已用 token', s: 'A', c: '' },
      { t: fmtInt(used), s: 'B', c: '' },
      { t: countdownText() + ' · ' + w5.t, s: 'C', c: w5.c },
    ]
  }
  return [
    { t: '本周额度剩余', s: 'A', c: '' },
    { t: rp.toFixed(1) + '%', s: 'P', c: rp < 20 ? C_WARN : C_TEXT },
    { t: countdownText() + ' · ' + w5.t, s: 'C', c: w5.c },
  ]
}
var LINES_CALM = [
  '今晚的月色很好，适合写代码。',
  '我在月球背面，帮你看着额度呢。',
  '这本书还差三页……看完就睡。',
  '嘘，小声点，月亮睡着了。',
  '要喝点月光茶吗？提神的。',
  '你的 bug，我帮你盯着它。',
  '熬夜的话，我陪你呀。',
  '据说对着月亮许愿，编译一次就过。'
]
var LINES_ANXIOUS = [
  '那个……额度快见底了，省着点用呀！',
  '只剩一点点了……要不要我帮你数着用？',
  '呜……再这样用下去，我要吃土了。',
  '额度不足两成了，后面的路省着点走……'
]
// 加权随机台词组：额度 <20% 时「着急」组权重反超日常组
function buildRandomGroups() {
  var rp = remainPercent()
  var low = rp !== null && rp < 20
  var groups = [
    { w: low ? 20 : 42, lines: buildStatusGroup },
    { w: low ? 6 : 16, lines: function () { return singleCenter('A', pickOne(LINES_CALM), '', true) } },
    { w: 8, lines: function () { return singleCenter('B', pickOne(['月兔娘...↓', 'Zzz...'])) } },
    { w: 6, lines: function () { return { gif: true } } }
  ]
  if (low) {
    groups.push({ w: 34, lines: function () { return singleCenter('A', pickOne(LINES_ANXIOUS), '', true) } })
  } else {
    groups.push({ w: 3, lines: function () { return singleCenter('A', pickOne(LINES_ANXIOUS), '', true) } })
  }
  groups.push({ w: 2, lines: function () { return singleCenter('B', '哦月月... ') } })
  return groups
}
function pickRandomLines() {
  var groups = buildRandomGroups()
  var total = 0
  var i
  for (i = 0; i < groups.length; i++) total += groups[i].w
  var r = Math.random() * total
  for (i = 0; i < groups.length; i++) {
    r -= groups[i].w
    if (r < 0) return groups[i].lines()
  }
  return groups[groups.length - 1].lines()
}
function applyBubbleLines(lines) {
  // 随机台词段：隐藏进度条与 5h 状态行，只排三行文字
  barEl.style.display = 'none'
  statusEl.style.display = 'none'
  // gif 台词组：只显示动图；动图素材缺失时降级为文字台词
  if (lines && lines.gif && !gifFailed) {
    gifEl.style.display = 'block'
    labelEl.style.display = 'none'
    amountEl.style.display = 'none'
    hintEl.style.display = 'none'
    return
  }
  gifEl.style.display = 'none'
  if (lines && lines.gif) {
    lines = singleCenter('A', pickOne(['今天没有动图给你看~', '呜呜，动图走丢了……', '月亮上信号不好，动图加载失败啦']), '', true)
  }
  var els = [labelEl, amountEl, hintEl]
  for (var i = 0; i < 3; i++) {
    var el = els[i]
    var ln = lines && lines[i]
    if (ln) {
      el.style.display = ''
      el.className = (BUBBLE_STYLE_CLASS[ln.s] || 'krw-label') + (ln.w ? ' krw-wrap' : '')
      el.textContent = ln.t
      el.style.color = ln.c || ''
    } else {
      el.style.display = 'none'
      el.textContent = ''
      el.style.color = ''
    }
  }
}

// ---------- 气泡打开/关闭/换内容（对齐原项目时序） ----------
var bubbleSwapTimer = null
var hintFadeTimer = null
var lastHintText = null
function setHint(text) {
  // 首次或气泡关闭时直接写，避免「先淡出再淡入」的闪断感
  if (text === lastHintText) return
  var first = lastHintText === null
  lastHintText = text
  if (first || !bubbleShown) { hintEl.textContent = text; return }
  hintEl.style.transition = 'opacity .18s ease'
  hintEl.style.opacity = '0'
  hintFadeTimer = setTimeout(function () {
    hintFadeTimer = null
    hintEl.textContent = text
    hintEl.style.opacity = '1'
    setTimeout(function () { hintEl.style.transition = ''; hintEl.style.opacity = '' }, 220)
  }, 190)
}
function swapBubbleContent(applyFn) {
  if (bubbleSwapTimer) { clearTimeout(bubbleSwapTimer); bubbleSwapTimer = null }
  textBox.style.transition = 'opacity .18s ease'
  textBox.style.opacity = '0'
  bubbleSwapTimer = setTimeout(function () {
    bubbleSwapTimer = null
    applyFn()
    textBox.style.opacity = '1'
    setTimeout(function () { textBox.style.transition = ''; textBox.style.opacity = '' }, 220)
  }, 190)
}
function restoreBubbleLines() {
  if (bubbleSwapTimer) { clearTimeout(bubbleSwapTimer); bubbleSwapTimer = null }
  if (hintFadeTimer) { clearTimeout(hintFadeTimer); hintFadeTimer = null }
  lastHintText = null
  textBox.style.transition = ''
  textBox.style.opacity = ''
  labelEl.style.display = ''
  labelEl.className = 'krw-label'
  labelEl.style.color = ''
  amountEl.style.display = ''
  amountEl.className = 'krw-amount'
  amountEl.style.color = ''
  hintEl.style.display = ''
  hintEl.className = 'krw-hint'
  hintEl.style.color = ''
  statusEl.style.display = ''
  barEl.style.display = ''
  gifEl.style.display = 'none'
  render()
}
function showBubble() {
  if (!bubbleOn) return
  if (costBubbleActive) return   // 消耗泡泡显示期间普通气泡不弹
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  bubbleShown = true
  bubbleRandomActive = false
  restoreBubbleLines()
  bubbleBox.classList.add('krw-bubble-open')
  bubbleTimer = setTimeout(hideBubble, BUBBLE_MS)
}
function hideBubble() {
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  if (bubbleSwapTimer) { clearTimeout(bubbleSwapTimer); bubbleSwapTimer = null }
  if (hintFadeTimer) { clearTimeout(hintFadeTimer); hintFadeTimer = null }
  textBox.style.transition = ''
  textBox.style.opacity = ''
  hintEl.style.transition = ''
  hintEl.style.opacity = ''
  bubbleRandomActive = false
  bubbleRandomLines = null
  bubbleShown = false
  bubbleBox.classList.remove('krw-bubble-open')
  // gif 靠 opacity 过渡淡出，等动画结束再 display:none
  setTimeout(function () { if (!bubbleShown) gifEl.style.display = 'none' }, 240)
}
bubbleBox.addEventListener('click', function (e) {
  e.stopPropagation()
  if (!bubbleShown) return
  if (costBubbleActive) { hideCostBubble(); return }
  if (bubbleRandomActive) {
    hideBubble()   // 再点关闭
  } else {
    // 首次点击：切加权随机台词，并重置 5 秒自动关闭计时
    bubbleRandomActive = true
    bubbleRandomLines = pickRandomLines()
    swapBubbleContent(function () { applyBubbleLines(bubbleRandomLines) })
    if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
    bubbleTimer = setTimeout(hideBubble, BUBBLE_MS)
  }
})

// ---------- 每轮对话消耗泡泡 ----------
function showCostBubble(tokens) {
  if (!bubbleOn || !turnCostOn) return
  if (costBubbleTimer) { clearTimeout(costBubbleTimer); costBubbleTimer = null }
  if (bubbleTimer) { clearTimeout(bubbleTimer); bubbleTimer = null }
  if (animId) { cancelAnimationFrame(animId); animId = null }
  costBubbleActive = true
  bubbleRandomActive = false
  bubbleShown = true
  lastHintText = null
  barEl.style.display = 'none'
  statusEl.style.display = 'none'
  gifEl.style.display = 'none'
  labelEl.style.display = ''
  labelEl.className = 'krw-label'
  labelEl.textContent = '上一轮消耗约'
  labelEl.style.color = ''
  amountEl.style.display = ''
  amountEl.className = 'krw-amount'
  amountEl.textContent = fmtInt(tokens) + ' token'
  amountEl.style.color = C_GOLD
  hintEl.style.display = 'none'
  hintEl.textContent = ''
  textBox.style.transition = ''
  textBox.style.opacity = ''
  bubbleBox.classList.add('krw-bubble-open')
  costBubbleTimer = turnCostCloseMs > 0 ? setTimeout(hideCostBubble, turnCostCloseMs) : null
}
function hideCostBubble() {
  if (costBubbleTimer) { clearTimeout(costBubbleTimer); costBubbleTimer = null }
  costBubbleActive = false
  hideBubble()
}

// ---------- 数字滚动 ----------
function animateAmount(from, to, suffix, duration) {
  if (costBubbleActive) return
  if (animId) cancelAnimationFrame(animId)
  if (from === null || !isFinite(from)) from = to
  if (from === to) {
    shown = to
    amountEl.textContent = to.toFixed(1) + suffix
    return
  }
  var startT = null
  function step(ts) {
    if (costBubbleActive) { animId = null; return }
    if (startT === null) startT = ts
    var t = Math.min(1, (ts - startT) / duration)
    var e = 1 - Math.pow(1 - t, 3)
    amountEl.textContent = (from + (to - from) * e).toFixed(1) + suffix
    if (t < 1) {
      animId = requestAnimationFrame(step)
    } else {
      animId = null
      shown = to
      amountEl.textContent = to.toFixed(1) + suffix
    }
  }
  animId = requestAnimationFrame(step)
}

// ---------- 渲染 ----------
function render() {
  if (costBubbleActive) return
  var u = state.usage
  if (state.status === 'error') {
    labelEl.textContent = 'Kimi 本周额度'
    amountEl.textContent = shown !== null ? shown.toFixed(1) + '%' : '--'
    barEl.style.display = 'none'
    setHint(state.message ? String(state.message).slice(0, 14) : '获取失败 · 点击兔娘重试')
    statusEl.textContent = ''
    return
  }
  if (!u) {
    labelEl.textContent = 'Kimi 本周额度'
    amountEl.textContent = '…'
    barEl.style.display = 'none'
    setHint('加载中…')
    statusEl.textContent = ''
    return
  }
  var rp = remainPercent()
  var w5 = window5hText()
  if (rp !== null) {
    labelEl.textContent = '本周额度剩余'
    amountEl.textContent = (shown !== null ? shown : rp).toFixed(1) + '%'
    barEl.style.display = ''
    barFillEl.style.width = rp + '%'
    barFillEl.classList.toggle('krw-bar-low', rp < 20)
    setHint(countdownText())
  } else {
    labelEl.textContent = '本周已用 token'
    amountEl.textContent = fmtInt(u.week && u.week.used_tokens)
    barEl.style.display = 'none'
    setHint(countdownText() + ' · 未设上限')
  }
  if (bubbleRandomActive && bubbleRandomLines) {
    applyBubbleLines(bubbleRandomLines)
  } else {
    statusEl.style.display = ''
    statusEl.textContent = w5.t
    statusEl.style.color = w5.c
  }
}

// ---------- 数据刷新 ----------
function refresh(manual) {
  if (busy) return
  busy = true
  if (manual || !state.usage) { state.status = 'loading'; render() }
  invoke('get_usage')
    .then(function (data) {
      if (data && typeof data === 'object' && data.week) {
        var prevRp = remainPercent()
        state.usage = data
        state.status = 'ok'
        state.message = ''
        // 加油包菜单项：fuel_pack 为 null 隐藏
        updateFuelRow(data.fuel_pack)
        // 每轮消耗：seq 递增且开关开 → 弹消耗泡泡（首次只对齐不弹）
        if (data.last_turn && typeof data.last_turn.seq === 'number') {
          if (lastTurnSeq >= 0 && data.last_turn.seq > lastTurnSeq && typeof data.last_turn.tokens === 'number') {
            showCostBubble(data.last_turn.tokens)
          }
          lastTurnSeq = data.last_turn.seq
        }
        var rp = remainPercent()
        if (rp !== null) {
          if (prevRp !== null && Math.abs(rp - prevRp) > 0.001) {
            if (!manual) showBubble()   // 额度变化：自动冒泡 + 滚动数字
            animateAmount(shown, rp, '%', ANIM_MS)
          } else if (animId === null) {
            shown = rp
          }
        }
        render()
      } else {
        state.status = 'error'
        state.message = '获取失败'
        render()
      }
    })
    .catch(function () {
      state.status = 'error'
      state.message = '获取失败 · 点击兔娘重试'
      render()
    })
    .then(function () { busy = false })
}
function updateFuelRow(fp) {
  if (fp === null || fp === undefined) { fuelRow.style.display = 'none'; return }
  var text
  if (typeof fp === 'number') {
    // 数值较小按人民币余额、较大按 token 数展示（后端格式未定前的防御性格式化）
    text = fp < 10000 ? '¥ ' + fp.toFixed(2) : fmtInt(fp) + ' token'
  } else if (typeof fp === 'object') {
    if (typeof fp.balance === 'number') text = '¥ ' + fp.balance.toFixed(2)
    else if (typeof fp.tokens === 'number') text = fmtInt(fp.tokens) + ' token'
    else text = '已开启'
  } else {
    text = String(fp)
  }
  fuelValue.textContent = text
  fuelRow.style.display = ''
}

// ---------- 菜单开关与定位 ----------
function toggleMenu() {
  menuOpen = !menuOpen
  if (menuOpen) positionMenu()
  menuBox.classList.toggle('krw-menu-open', menuOpen)
  if (menuOpen) menuBtn.classList.add('krw-menu-btn-visible')
}
function closeMenu() {
  menuOpen = false
  menuBox.classList.remove('krw-menu-open')
}
function positionMenu() {
  try {
    var b = menuBtn.getBoundingClientRect()
    var onLeft = state.h === 'left'
    // 窗口尺寸：Tauri 下窗口就是挂件本身（可能只有 200px），mock 下用视口
    var vw = isTauri ? rootSize() : (window.innerWidth || 1280)
    var vh = isTauri ? rootSize() : (window.innerHeight || 800)
    // 横向：窗口可能比菜单最小宽度还窄，同步收窄并允许横向滚动
    var maxW = Math.max(0, vw - 8)
    menuBox.style.minWidth = Math.min(210, maxW) + 'px'
    menuBox.style.maxWidth = maxW + 'px'
    // 纵向：按钮上方空间够就向上展开；不够就从窗口顶部向下展开
    // （盖住角色无妨，总比被窗口裁掉强），内容超出可滚动
    var need = menuBox.scrollHeight || 300
    var openUp = b.top >= Math.min(need, vh - 8) + 8
    if (openUp) {
      menuBox.style.bottom = Math.max(0, vh - b.top) + 'px'
      menuBox.style.top = 'auto'
      menuBox.style.maxHeight = ''
    } else {
      menuBox.style.top = '4px'
      menuBox.style.bottom = 'auto'
      menuBox.style.maxHeight = Math.max(0, vh - 8) + 'px'
    }
    // 贴角色所在一侧
    if (onLeft) {
      menuBox.style.left = Math.max(4, b.left) + 'px'
      menuBox.style.right = 'auto'
      menuBox.style.transformOrigin = (openUp ? 'bottom' : 'top') + ' left'
    } else {
      menuBox.style.right = Math.max(4, vw - b.right) + 'px'
      menuBox.style.left = 'auto'
      menuBox.style.transformOrigin = (openUp ? 'bottom' : 'top') + ' right'
    }
  } catch (err) {}
}

// ---------- 命中测试：角色裁成圆形，按圆判定 ----------
function isRabbitHit(e) {
  try {
    var r = img.getBoundingClientRect()
    if (!r || r.width <= 0 || r.height <= 0) return false
    var dx = (e.clientX - (r.left + r.width / 2)) / (r.width / 2)
    var dy = (e.clientY - (r.top + r.height / 2)) / (r.height / 2)
    return dx * dx + dy * dy <= 1
  } catch (err) { return true }
}

// ---------- 拖拽：pointer 跟踪 + Tauri 移动 OS 窗口 / mock 挪 DOM ----------
function onDocPointerDown(e) {
  if (e.target && e.target.closest) {
    if (e.target.closest('.krw-bubble') || e.target.closest('.krw-menu') || e.target.closest('.krw-menu-btn')) return
  }
  if (menuOpen) { closeMenu(); return }
  if (e.button !== 0 && e.pointerType === 'mouse') return
  if (!isRabbitHit(e)) return
  try { e.preventDefault(); e.stopPropagation() } catch (err) {}
  // Tauri 用 screen 坐标增量（窗口跟着动），mock 用 client 坐标增量
  drag = {
    active: true,
    startX: isTauri ? e.screenX : e.clientX,
    startY: isTauri ? e.screenY : e.clientY,
    origLeft: state.left,
    origTop: state.top,
    moved: false
  }
  if (winAnimId) { cancelAnimationFrame(winAnimId); winAnimId = null }
  root.classList.add('krw-dragging')
  pressDown()
  setWidgetCursor('grabbing')
  document.addEventListener('pointermove', onDocPointerMove, true)
  document.addEventListener('pointerup', onDocPointerUp, true)
  document.addEventListener('pointercancel', onDocPointerCancel, true)
}
function onDocPointerMove(e) {
  if (!drag || !drag.active) return
  var px = isTauri ? e.screenX : e.clientX
  var py = isTauri ? e.screenY : e.clientY
  var dx = px - drag.startX
  var dy = py - drag.startY
  if (dx * dx + dy * dy >= CLICK_SQ) drag.moved = true
  placeWidget(drag.origLeft + dx, drag.origTop + dy, false)
}
function onDocPointerUp(e) { endDrag(e, true) }
function onDocPointerCancel(e) { endDrag(e, false) }
function endDrag(e, clickAllowed) {
  if (!drag || !drag.active) return
  var d = drag
  drag.active = false
  document.removeEventListener('pointermove', onDocPointerMove, true)
  document.removeEventListener('pointerup', onDocPointerUp, true)
  document.removeEventListener('pointercancel', onDocPointerCancel, true)
  pressUp()
  root.classList.remove('krw-dragging')
  setWidgetCursor(isRabbitHit(e) ? 'grab' : '')
  if (clickAllowed && !d.moved) {
    // 单击兔娘：手动刷新 + 冒泡
    showBubble()
    refresh(true)
    return
  }
  // 松手吸附：按中心点象限滑向对应边
  var t = snapTarget(state.left, state.top, rootSize())
  state.h = t.h
  state.v = t.v
  placeWidget(t.x, t.y, true)
  persistPos()
}
document.addEventListener('pointerdown', onDocPointerDown, true)
document.addEventListener('click', function (e) {
  // 拦截角色区域内的 click，避免穿透触发下方元素
  if (!isRabbitHit(e)) return
  try { e.preventDefault(); e.stopPropagation() } catch (err) {}
}, true)

var widgetCursor = ''
function setWidgetCursor(v) {
  if (v !== widgetCursor) {
    widgetCursor = v
    try { document.body.style.cursor = v } catch (err) {}
  }
}
document.addEventListener('pointermove', function (e) {
  if (drag && drag.active) { setWidgetCursor('grabbing'); return }
  var el = null
  try { el = document.elementFromPoint(e.clientX, e.clientY) } catch (err) {}
  if (el && el.closest && (el.closest('.krw-bubble') || el.closest('.krw-menu') || el.closest('.krw-menu-btn'))) {
    setWidgetCursor('')
    menuBtn.classList.add('krw-menu-btn-visible')
    return
  }
  var over = isRabbitHit(e)
  setWidgetCursor(over ? 'grab' : '')
  menuBtn.classList.toggle('krw-menu-btn-visible', over || menuOpen)
}, true)

// ---------- 启动 ----------
setupSound()
if (isTauri) tauriSetSize(rootSize())
express()
render()
// 先读配置恢复（含位置），再开始刷新
invoke('get_config')
  .then(function (d) { applyConfig(d || {}) })
  .catch(function () {
    state.h = 'right'; state.v = 'bottom'
    var sr = screenRect()
    placeWidget(sr.x + sr.w - rootSize(), sr.y + sr.h - rootSize(), false)
  })
  .then(function () { refresh(false) })
setInterval(function () { refresh(false) }, REFRESH_MS)
// 倒计时文字每分钟更新一次（仅刷新文案，不打断滚动动画）
setInterval(function () {
  if (!state.usage || costBubbleActive || bubbleRandomActive) return
  lastHintText = null   // 强制 setHint 重新写入
  render()
}, 60000)
})()
