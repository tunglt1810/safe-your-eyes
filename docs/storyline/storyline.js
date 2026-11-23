// One workday with AI, as a comic page: nine panels read left to right, top to
// bottom. Each panel is a state; panel 6 is the detour when skips run out.
const ABOUT = 'a story, one panel at a time';
const COLS = [[72, 486], [506, 902], [922, 1328]], ROWS = [[262, 566], [588, 892], [914, 1218]];
const IDS = ['morning', 'strain', 'chime', 'stretch', 'sprint', 'locked', 'calm', 'shipped', 'night'];
const CAPS = {
  morning: '09:00. Coffee, AI, go.', strain: '11:00. Forgot to blink.', chime: 'Ding. 15 seconds.',
  stretch: 'Stand up. An idea lands.', sprint: '15:00. Just one more prompt...', locked: '2/2. No skip this time.',
  calm: 'Eyes shut. Mind opens.', shipped: 'Shipped. Eyes still fresh.', night: 'Quit. See you at 9.',
};
const NAMES = {
  morning: 'coffee, AI, go', strain: 'forgot to blink', chime: 'a 15-second pause', stretch: 'an idea lands',
  sprint: 'one more prompt', locked: 'no skip this time', calm: 'eyes shut, mind open', shipped: 'shipped, eyes fresh', night: 'quit, see you at 9',
};
const MUG = '#c0473a', SCREEN = '#2f3442', CODE = ['#8fae84', '#e3c77a', '#8ea6c8', '#e6c3bb'];
let AT = 'morning';
let V = { due: false, skips: 0, rested: false };

function frame(id) {
  const i = IDS.indexOf(id), [x0, x1] = COLS[i % 3], [y0, y1] = ROWS[Math.floor(i / 3)], k = (i * 7) % 5 - 2;
  return [[x0, y0 + k], [x1, y0 - k], [x1 - k, y1], [x0 + k, y1]];
}
const mid = id => { const f = frame(id); return [(f[0][0] + f[1][0]) / 2, (f[0][1] + f[3][1]) / 2]; };

// the red mug: it travels from panel to panel, and steams while it is full
function mug(sx, sy, { s = 1, full = true, steam = false } = {}) {
  out.push(`<g transform="translate(${n1(sx)} ${n1(sy)}) scale(${s})">`);
  shape([[-9, -20], [9, -20], [8, 0], [-8, 0]], MUG, { w: 1.1 });
  line(spline([[9, -16], [16, -14], [16, -6], [8.5, -4]], false, 2), { w: 2, over: 0 });
  shape(oval(0, -20, 9, 2.6, 16), full ? '#6b4a35' : '#f3ead8', { w: 0.9 });
  if (steam) group('class="steam"', () => { for (const dx of [-4, 3]) line(spline([[dx, -26], [dx + 3, -32], [dx - 1, -38], [dx + 2, -44]], false, 2), { w: 1, col: '#9aa0a8', over: 0 }); });
  out.push('</g>');
}

function bubble(x, y, text, tx, ty) {
  const w = textW(text, 16) + 26, h = 34;
  out.push(`<path d="${pathOf([[x - 8, y + h / 2 - 2], [tx, ty], [x + 8, y + h / 2 - 2]], true)}" fill="#fffdf6" stroke="${INK}" stroke-width="1"/>`);
  out.push(`<rect x="${n1(x - w / 2)}" y="${n1(y - h / 2)}" width="${n1(w)}" height="${h}" rx="16" fill="#fffdf6" stroke="${INK}" stroke-width="1.1"/>`);
  out.push(`<rect x="${n1(x - 9)}" y="${n1(y + h / 2 - 3)}" width="18" height="4" fill="#fffdf6"/>`);
  _text(x, y + 6, text, { size: 16, anchor: 'middle' });
}

function room(a, h, wallR, wallL, floor) {
  flat([P(0, 0), P(a, 0), P(a, a), P(0, a)], floor);
  flat([P(0, 0, 0), P(a, 0, 0), P(a, 0, h), P(0, 0, h)], wallR);
  flat([P(0, 0, 0), P(0, a, 0), P(0, a, h), P(0, 0, h)], wallL);
  rule([[P(0, 0, 0), P(0, 0, h)], [P(0, 0, 0), P(a, 0, 0)], [P(0, 0, 0), P(0, a, 0)]], INK, 0.9);
}
const winL = (y, z, w, h, col) => { const q = [P(0, y, z), P(0, y + w, z), P(0, y + w, z + h), P(0, y, z + h)]; flat(q, col); rim(q, 0.9); return q; };

// the face of a person drawn with who(): eyes shut, or tired and red
function face(x, y, { look = 1, sit = 0, mood = 'shut' } = {}) {
  const [bx, by] = P(x, y), s = 2.1, fx = look < 0 ? -1 : 1;
  for (const sd of [-1, 1]) {
    const ex = bx + (sd * 2.1 + 0.6) * s * fx, ey = by + (-30.2 + 6 * sit) * s;
    if (mood === 'shut') {
      flat(oval(ex, ey, 1.6, 1.6, 12), SKIN);
      line([[ex - 1.6, ey - 0.4], [ex, ey + 0.9], [ex + 1.6, ey - 0.4]], { w: 1.5, over: 0, amp: 0 });
    } else out.push(`<circle cx="${n1(ex)}" cy="${n1(ey)}" r="2.6" fill="none" stroke="${RED}" stroke-width="1"/>`);
  }
  if (mood === 'tired') shape([[bx + 16 * fx, by - 68], [bx + 19 * fx, by - 60], [bx + 13 * fx, by - 60]], '#a9c4d6', { w: 0.8 });
}

function bulb(sx, sy) {
  shape(oval(sx, sy, 9, 10, 18), PAL.butter, { w: 1.1 });
  shape([[sx - 4, sy + 9], [sx + 4, sy + 9], [sx + 3, sy + 15], [sx - 3, sy + 15]], PAL.slate, { w: 0.8 });
  for (let k = 0; k < 7; k++) { const a = Math.PI * (1.05 + k / 7.5); line([[sx + Math.cos(a) * 14, sy + Math.sin(a) * 14], [sx + Math.cos(a) * 21, sy + Math.sin(a) * 21]], { w: 1.4, col: PAL.mustard, over: 0, amp: 0 }); }
}

// the desk scene that most panels share. The screen stands on the plane y = 24.
let SCR = [];
function desk({ wallR = '#efe3cf', wallL = '#e4d6bf', floor = '#d6c2a2', sky = '#cfe6ee', sun = true, screen = 'code', who: person = null, cat: kitty = null, cup = null, extra = null } = {}) {
  SCR = [P(76, 24, 48), P(172, 24, 48), P(172, 24, 112), P(76, 24, 112)];
  room(230, 130, wallR, wallL, floor);
  const q = winL(70, 52, 90, 60, sky);
  clipped(q, () => { const [sx, sy] = P(0, 140, 96); out.push(`<circle cx="${n1(sx)}" cy="${n1(sy)}" r="13" fill="${sun ? PAL.mustard : '#fff6c8'}"/>`); });
  rule([[P(0, 115, 52), P(0, 115, 112)]], INK, 0.9);
  put(20, 0, () => { box(150, 4, 0, 70, 22, 110, { left: PAL.wood, top: PAL.tan, marks: false }); for (const [z, c] of [[16, PAL.rose], [52, PAL.sky], [88, PAL.sage]]) box(156, 8, z, 58, 12, 16, { left: c, marks: false }); });
  put(60, 0, () => {
    box(70, 6, 0, 110, 44, 40, { left: PAL.wood, top: PAL.tan, marks: false });
    box(120, 20, 40, 10, 6, 8, { left: PAL.slate, marks: false });
    flat(SCR, screen === 'off' ? '#3b3f4b' : SCREEN); rim(SCR, 1.2);
    clipped(SCR, () => SCREENS[screen]());
    box(92, 34, 40, 60, 12, 2, { left: PAL.grey, top: PAL.stone, marks: false });
  });
  put(90, 0, () => box(160, 6, 40, 8, 8, 6, { left: PAL.sage, marks: false }));
  if (cup) put(95, 1, () => { const [sx, sy] = P(...cup.at); mug(sx, sy, { s: 0.8, ...cup }); });
  if (person) put(240, 0, () => {
    if (person.sit) cyl(125, 105, 0, 16, 20, PAL.navy, { cap: PAL.blue });
    who(125, 105, { shirt: PAL.sage, pants: PAL.navy, hair: 'short', look: -1, ...person });
    if (person.mood) face(125, 105, { look: -1, sit: person.sit || 0, mood: person.mood });
  });
  if (kitty) put(kitty.depth || 300, 0, () => cat(...kitty.at, kitty.z || 0, '#e3c77a'));
  put(330, 0, () => { box(30, 170, 0, 34, 34, 28, { left: PAL.sage, top: PAL.mint, marks: false }); tree(47, 187, 16, 28, PAL.leaf); });
  if (extra) extra();
  flush();
}

const scr = (u, v) => { const [a, b, , d] = SCR; return [a[0] + (b[0] - a[0]) * u + (d[0] - a[0]) * v, a[1] + (b[1] - a[1]) * u + (d[1] - a[1]) * v]; };
function codeLines(n, busy = false) {
  for (let k = 0; k < n; k++) {
    const v = 0.86 - k * (0.72 / n), u0 = 0.08 + (k % 3) * 0.05, u1 = u0 + 0.25 + ((k * 37) % 50) / 100;
    line([scr(u0, v), scr(Math.min(u1, 0.92), v)], { w: busy ? 2.2 : 2.6, col: CODE[k % 4], over: 0, amp: 0 });
  }
}
function card(t1, t2, { lock = false } = {}) {
  flat([scr(0.08, 0.06), scr(0.92, 0.06), scr(0.92, 0.94), scr(0.08, 0.94)], '#3d3530');
  const [cx, cy] = scr(0.5, 0.62);
  _text(cx, cy + 4, t1, { size: 13, col: '#f3ead8', anchor: 'middle' });
  if (t2) {
    for (const u of [0.3, 0.7]) {
      const [bx, by] = scr(u, 0.22);
      out.push(`<rect x="${n1(bx - 18)}" y="${n1(by - 9)}" width="36" height="14" rx="5" fill="${lock ? '#6d6560' : PAL.sage}" opacity="${lock ? 0.6 : 1}"/>`);
    }
  }
  if (lock) { const [lx, ly] = scr(0.5, 0.22); out.push(`<rect x="${n1(lx - 5)}" y="${n1(ly - 4)}" width="10" height="8" rx="1.5" fill="${PAL.mustard}" stroke="${INK}" stroke-width=".8"/><path d="M${n1(lx - 3)} ${n1(ly - 4)}v-3a3 3 0 0 1 6 0v3" fill="none" stroke="${INK}" stroke-width=".9"/>`); }
}
const SCREENS = {
  code: () => codeLines(6),
  busy: () => { codeLines(9, true); for (let k = 0; k < 4; k++) { const [x, y] = scr(0.06 + k * 0.2, 0.96); out.push(`<rect x="${n1(x)}" y="${n1(y - 4)}" width="14" height="6" fill="${CODE[k]}"/>`); } },
  pause: () => card('00:15', 'buttons'),
  sprint: () => { codeLines(8, true); card(`skips ${Math.min(V.skips, 2)}/2`, 'buttons'); },
  locked: () => card('00:15', 'buttons', { lock: true }),
  calm: () => card('00:09', null),
  shipped: () => { const [cx, cy] = scr(0.5, 0.55); out.push(`<circle cx="${n1(cx)}" cy="${n1(cy)}" r="14" fill="${PAL.leaf}"/>`); line([[cx - 7, cy], [cx - 2, cy + 5], [cx + 7, cy - 6]], { w: 2.4, col: '#fffdf6', over: 0, amp: 0 }); },
  off: () => {},
};

const SCENES = {
  morning() {
    desk({ who: { sit: 1, arm: 'out' }, cup: { at: [168, 40, 40], full: true, steam: true }, cat: { at: [80, 170], depth: 250 } });
    bubble(...P(76, 150, 128), 'Let’s build!', ...P(118, 105, 62));
  },
  strain() {
    desk({ screen: 'busy', who: { sit: 1, arm: 'hold', mood: 'tired' }, cup: { at: [168, 40, 40], full: false }, cat: { at: [80, 170], depth: 250 } });
    if (V.due) {
      const [nx, ny] = scr(0.5, 0.5), w = textW('Break in 10 s', 13) + 20;
      out.push(`<rect x="${n1(nx - w / 2)}" y="${n1(ny - 12)}" width="${n1(w)}" height="22" rx="6" fill="#fffdf6" stroke="${RED}" stroke-width="1.2"/>`);
      _text(nx, ny + 4, 'Break in 10 s', { size: 13, col: RED, anchor: 'middle' });
    } else bubble(...P(76, 150, 128), 'My eyes...', ...P(118, 105, 62));
  },
  chime() {
    desk({ screen: 'pause', who: { sit: 1, arm: 'down', mood: 'shut' }, cup: { at: [168, 40, 40], full: false }, cat: { at: [80, 170], depth: 250 } });
    const [bx, by] = P(150, 24, 124);
    for (const r of [10, 18]) out.push(`<path d="M${n1(bx - r)} ${n1(by)}a${r} ${r} 0 0 1 ${2 * r} 0" fill="none" stroke="${RED}" stroke-width="1.2"/>`);
    _text(bx, by - 22, 'ding', { size: 14, col: RED, anchor: 'middle' });
  },
  stretch() {
    desk({ screen: 'calm', who: { sit: 0, arm: 'up' }, cup: { at: [168, 40, 40], full: true, steam: true }, cat: { at: [70, 160], depth: 230 } });
    const [sx, sy] = P(125, 105, 92); bulb(sx + 4, sy - 8);
    bubble(...P(76, 150, 128), 'Oh! That’s it.', ...P(118, 105, 70));
  },
  sprint() {
    desk({ sky: '#f2d9b5', screen: 'sprint', who: { sit: 1, arm: 'out' }, cup: { at: [168, 40, 40], full: false }, cat: { at: [80, 170], depth: 250 } });
    bubble(...P(76, 150, 128), V.skips ? 'Skip. Again.' : 'Just one more.', ...P(118, 105, 62));
  },
  locked() {
    desk({ sky: '#f2d9b5', screen: 'locked', who: { sit: 1, arm: 'hold' }, cup: { at: [168, 40, 40], full: false }, cat: { at: [100, 130], depth: 260 } });
    bubble(...P(76, 150, 128), 'Okay, okay.', ...P(118, 105, 62));
  },
  calm() {
    desk({ sky: '#f2d9b5', screen: 'calm', who: { sit: 1, arm: 'down', mood: 'shut' }, cup: { at: [168, 40, 40], full: true, steam: true }, cat: { at: [100, 130], depth: 260 } });
    const [sx, sy] = P(125, 105, 100);
    for (const [dx, dy, r] of [[-18, -8, 3], [-26, -20, 5]]) out.push(`<circle cx="${n1(sx + dx)}" cy="${n1(sy + dy)}" r="${r}" fill="#fffdf6" stroke="${INK}" stroke-width=".9"/>`);
    out.push(`<ellipse cx="${n1(sx - 56)}" cy="${n1(sy - 44)}" rx="30" ry="18" fill="#fffdf6" stroke="${INK}" stroke-width="1"/>`);
    bulb(sx - 56, sy - 48);
  },
  shipped() {
    desk({ sky: '#f6c9a0', screen: 'shipped', who: { sit: 0, arm: 'up' }, cup: { at: [168, 40, 40], full: false }, cat: { at: [70, 160], depth: 230 } });
    const f = frame('shipped');
    for (let k = 0; k < 22; k++) { const x = f[0][0] + 40 + rnd() * 320, y = f[0][1] + 50 + rnd() * 110; out.push(`<rect x="${n1(x)}" y="${n1(y)}" width="5" height="3" fill="${CODE[k % 4]}" transform="rotate(${n1(rnd() * 90)} ${n1(x)} ${n1(y)})"/>`); }
    bubble(...P(76, 150, 128), 'Shipped!', ...P(118, 105, 70));
  },
  night() {
    desk({ wallR: '#3d4462', wallL: '#353b57', floor: '#4a4f62', sky: '#202640', sun: false, screen: 'off', cup: { at: [168, 40, 40], full: false }, cat: { at: [125, 105], depth: 240 },
      extra: () => put(250, 0, () => { box(186, 60, 0, 30, 30, 8, { left: PAL.slate, marks: false }); cyl(201, 75, 8, 2, 70, PAL.slate); turn(201, 75, 78, [[4, 0], [14, 16]], PAL.butter, { cap: PAL.mustard }); }) });
    const [lx, ly] = P(201, 75, 88); out.push(`<circle cx="${n1(lx)}" cy="${n1(ly + 40)}" r="70" fill="#fff6c8" opacity=".14"/>`);
    const [tx, ty] = P(124, 24, 82), w = textW('queue saved', 12) + 18;
    out.push(`<rect x="${n1(tx - w / 2)}" y="${n1(ty - 11)}" width="${n1(w)}" height="20" rx="6" fill="#fffdf6" stroke="${INK}" stroke-width="1"/>`);
    _text(tx, ty + 4, 'queue saved', { size: 12, anchor: 'middle' });
  },
};

function panel(id, faint) {
  const f = frame(id), [x0, y0] = f[0], i = IDS.indexOf(id);
  group(`data-state="${id}"`, () => {
    if (faint) out.push('<g opacity=".42">');
    fill(f, id === 'night' ? '#3a405c' : '#f6efe0', { amp: 0.2 });
    clipped(f, () => {
      OX = mid(id)[0] - 22; OY = y0 + 92;
      SCENES[id]();
      OX = 700; OY = 0;
    });
    rim(f, 1.8);
    const w = textW(CAPS[id], 15) + 44;
    out.push(`<rect x="${n1(x0 + 10)}" y="${n1(y0 + 10)}" width="${n1(w)}" height="30" fill="#fffdf6" stroke="${INK}" stroke-width="1"/>`);
    _text(x0 + 22, y0 + 31, String(i + 1), { size: 15, col: RED });
    _text(x0 + 38, y0 + 31, CAPS[id], { size: 15 });
    if (faint) out.push('</g>');
    out.push('<g class="on">');
    line([...f, f[0], f[1]], { w: 4, col: RED, amp: 0.4, over: 0 });
    nowTag(f[1][0] - 34, f[1][1] - 6);
    out.push('</g>');
  });
}

// the red route through the gutters. Sprint goes on to 6 when the skips run
// out, or down to 7; panel 6 comes back into 7 when the eyes close.
function gutters() {
  const g = (a, b) => flowArrow([a, b], { w: 2 });
  for (let r = 0; r < 3; r++) for (let c = 0; c < 2; c++) {
    const y = (ROWS[r][0] + ROWS[r][1]) / 2 + 60, x = (COLS[c][1] + COLS[c + 1][0]) / 2;
    g([x - 8, y], [x + 10, y]);
  }
  const y0 = (ROWS[0][1] + ROWS[1][0]) / 2;
  out.push(`<path d="M${COLS[2][0] + 200} ${y0}H${COLS[0][0] + 200}" fill="none" stroke="${RED}" stroke-width="2" stroke-dasharray="1 7" stroke-linecap="round"/>`);
  const by = (ROWS[1][1] + ROWS[2][0]) / 2;
  flowArrow([[COLS[1][0] + 60, ROWS[1][1] - 2], [COLS[1][0] + 60, by - 5], [COLS[0][0] + 200, by - 5], [COLS[0][0] + 200, ROWS[2][0] + 2]], { w: 2 });
  flowArrow([[COLS[2][0] + 120, ROWS[1][1] + 2], [COLS[2][0] + 120, by + 5], [COLS[0][0] + 300, by + 5], [COLS[0][0] + 300, ROWS[2][0] + 4]], { dashed: true, w: 2 });
}

function draw(seed) {
  begin(seed);
  OX = 700; OY = 0;
  const now = IDS.indexOf(AT);
  for (const id of IDS) panel(id, IDS.indexOf(id) > now);
  gutters();
  group('id="token"', () => { const f = frame(AT); mug(f[2][0] - 26, f[2][1] - 24, { s: 0.7 }); });
  title('a comic strip', 'Close your eyes');
  subtitle('open your mind: one workday with AI, and the pauses that keep the eyes fresh');
  status(NAMES[AT], `break ${V.due ? 'due' : 'not yet'} · skips ${V.skips}/2 · eyes ${V.rested ? 'rested' : 'open'}`);
  const by = (ROWS[1][1] + ROWS[2][0]) / 2;
  for (const [x, y, t] of [[COLS[0][0] + 262, by - 5, '[under 2 skips]'], [COLS[1][0] + 300, by + 5, 'close eyes']]) {
    out.push(`<rect x="${n1(x - textW(t, 13) / 2 - 6)}" y="${n1(y - 9)}" width="${n1(textW(t, 13) + 12)}" height="18" fill="${PAPER}"/>`);
    _text(x, y + 5, t, { size: 13, col: RED, anchor: 'middle' });
  }
  footnote('Illustrative: one made-up workday. Breaks use the Safe Your Eyes defaults: 15 s every 15 min, 60 s every 75 min, skip limit 2.');
  finish();
}

function init(ctx) {
  const $ = id => document.getElementById(id);
  const redraw = () => { draw(20); ctx.state(AT); };
  const go = (to, change = () => {}) => {
    const a = frame(AT), b = frame(to), A = at(a[2][0] - 26, a[2][1] - 24), B = at(b[2][0] - 26, b[2][1] - 24);
    const via = IDS.indexOf(to) === IDS.indexOf(AT) + 1 && Math.floor(IDS.indexOf(to) / 3) !== Math.floor(IDS.indexOf(AT) / 3) ? [A, at(a[2][0] - 26, a[3][1] + 10), at(b[0][0] + 40, b[0][1] - 10), B] : [A, B];
    // the camera rides with the mug, then closes in on the new panel as it comes in
    walk(ctx, $('token'), via.map(p => [...p, 0]), { ms: via.length > 2 ? 2600 : 1700, follow: 980, then: () => {
      change(); AT = to; crossfade(ctx, redraw, 800);
      ctx.focus(...mid(to), 2.3);
    } });
  };
  const NEXT = { morning: 'strain', chime: 'stretch', stretch: 'sprint', shipped: 'night' };
  ctx.button('next step', () => {
    if (AT === 'strain') {
      if (!V.due) return ctx.no('Not yet. The next break is a few minutes away.');
      ctx.say(CAPS.chime); return go('chime');
    }
    if (AT === 'sprint') {
      if (V.skips >= 2) { ctx.say('Two skips in a row. Skip and Postpone lock.'); return go('locked'); }
      ctx.say('The break comes. This time, the eyes close.'); return go('calm', () => { V.rested = false; });
    }
    if (AT === 'locked') return ctx.no('Skip is locked. Close your eyes.');
    if (AT === 'calm') {
      if (!V.rested) return ctx.no('Close your eyes first. 15 seconds.');
      ctx.say(CAPS.shipped); return go('shipped');
    }
    if (AT === 'night') { ctx.say('Another day. The saved queue picks up at nine.'); return go('morning', () => { V = { due: false, skips: 0, rested: false }; }); }
    const to = NEXT[AT]; ctx.say(CAPS[to]); go(to);
  });
  ctx.button('break due', () => {
    if (AT !== 'strain') return ctx.no('No break is due here.');
    if (V.due) return ctx.no('The notice is up. Ten seconds to go.');
    V.due = true; crossfade(ctx, redraw); ctx.say('A soft chime. Break in 10 seconds.');
  });
  ctx.button('close eyes', () => {
    if (AT === 'locked') { ctx.say('Eyes shut. Skips back to 0.'); return go('calm', () => { V.rested = true; V.skips = 0; }); }
    if (AT !== 'calm') return ctx.no(AT === 'chime' ? 'They are closed. Breathe.' : 'Wait for the break.');
    if (V.rested) return ctx.no('Eyes closed. Breathe.');
    V.rested = true; V.skips = 0; crossfade(ctx, redraw); ctx.say('Fifteen quiet seconds. The idea gets clearer.');
  });
  ctx.button('skip', () => {
    if (AT !== 'sprint') return ctx.no('Skip works only on a break.');
    if (V.skips >= 2) return ctx.no('2/2. Skip is locked.');
    V.skips++; crossfade(ctx, redraw); ctx.say(`Skipped. ${V.skips}/2.`);
  });
  ctx.hint('Press Play all, or next step. Skip twice in the afternoon, and see.');
  redraw();

  const STORY = ['morning', 'strain', 'strain', 'chime', 'stretch', 'sprint', 'sprint', 'sprint', 'locked', 'calm', 'shipped', 'night'];
  let si = 0;
  return {
    goto(id, play) {
      let j = play ? STORY.indexOf(id, si + 1) : -1;
      if (j < 0) j = STORY.indexOf(id);
      si = j;
      V = { due: j >= 2, skips: j >= 9 ? 0 : Math.max(0, Math.min(2, j - 5)), rested: j >= 9 };
      ctx.say(CAPS[id]);
      if (play && id !== AT) go(id);
      else { AT = id; redraw(); }
    },
    tick(t) {
      for (const el of document.querySelectorAll('.steam')) el.setAttribute('transform', `translate(0 ${n1(-((t * 6) % 6))})`);
    },
    story: STORY,
  };
}
