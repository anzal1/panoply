import * as THREE from 'three';
import { TOOLS, search, paintCard, paintFile, fontsReady, reduced, isPhone, clamp, lerp, sstep, damp, rnd } from './tools.js';
import { createBar } from './bar.js';

const phone = isPhone(), RM = reduced(), REC = /[?&]rec=1/.test(location.search);
const canvas = document.getElementById('c');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: !REC && !phone, powerPreference: 'high-performance' });
renderer.setPixelRatio(Math.min(devicePixelRatio || 1, phone ? 1.5 : 2));
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.05;
if (!phone) { renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap; }

const BG = 0x0a0806;
const scene = new THREE.Scene();
scene.background = new THREE.Color(BG);
scene.fog = new THREE.FogExp2(BG, phone ? 0.03 : 0.026);
const camera = new THREE.PerspectiveCamera(phone ? 62 : 50, 1, 0.1, 120);
const CX = 0, CZ = 8;               // centre of the hanging arcs
const ZBASE = phone ? 11.5 : 8;     // camera rest distance

// ---------- light: a museum, one warm key, a cool fill, a dim room ----------
scene.add(new THREE.HemisphereLight(0x3a342a, 0x0a0806, 0.55));
const key = new THREE.SpotLight(0xffe2b8, 2100, 0, 0.62, 0.9, 2);
key.position.set(-6, 15, 19); key.target.position.set(0, 0, -2);
scene.add(key, key.target);
if (!phone) {
  key.castShadow = true; key.shadow.mapSize.set(REC ? 1024 : 2048, REC ? 1024 : 2048);
  key.shadow.bias = -0.0005; key.shadow.normalBias = 0.03; key.shadow.camera.near = 6; key.shadow.camera.far = 70; key.shadow.radius = 5;
}
const fill = new THREE.SpotLight(0xb9ccff, 520, 0, 0.7, 1, 2);
fill.position.set(10, 8, 18); fill.target.position.set(0, 1, -4);
scene.add(fill, fill.target);

// room: a curved back wall and a polished floor, both receive the cards' shadows
const wall = new THREE.Mesh(new THREE.CylinderGeometry(30, 30, 46, 64, 1, true), new THREE.MeshStandardMaterial({ color: 0x3a342c, roughness: 0.92, side: THREE.BackSide }));
wall.position.set(CX, 6, CZ); wall.receiveShadow = true; scene.add(wall);
const floor = new THREE.Mesh(new THREE.PlaneGeometry(140, 140), new THREE.MeshStandardMaterial({ color: 0x1a1510, roughness: 0.28, metalness: 0.15 }));
floor.rotation.x = -Math.PI / 2; floor.position.y = -3.4; floor.receiveShadow = true; scene.add(floor);

// visible beam of the key light (a soft additive cone)
{
  const L = key.position.distanceTo(key.target.position), R = Math.tan(0.5) * L;
  const g = new THREE.ConeGeometry(R, L, 48, 1, true);
  const m = new THREE.ShaderMaterial({
    transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, fog: false,
    uniforms: { c: { value: new THREE.Color(0xffe2b8) } },
    vertexShader: 'varying vec2 vUv;varying vec3 vN;varying vec3 vV;void main(){vUv=uv;vN=normalize(normalMatrix*normal);vec4 mv=modelViewMatrix*vec4(position,1.);vV=-mv.xyz;gl_Position=projectionMatrix*mv;}',
    fragmentShader: 'uniform vec3 c;varying vec2 vUv;varying vec3 vN;varying vec3 vV;void main(){float f=abs(dot(normalize(vN),normalize(vV)));float a=pow(f,2.2)*pow(vUv.y,2.0)*0.06;gl_FragColor=vec4(c*a,a);}'
  });
  const cone = new THREE.Mesh(g, m);
  cone.position.copy(key.position).add(key.target.position).multiplyScalar(0.5);
  cone.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), key.position.clone().sub(key.target.position).normalize());
  cone.renderOrder = 2; if (!phone) scene.add(cone);
}

// dust in the beam
let dust = null, dustPos = null;
if (!phone) {
  const N = 520; dustPos = new Float32Array(N * 3);
  for (let i = 0; i < N; i++) { dustPos[i * 3] = (rnd(i) - .5) * 30; dustPos[i * 3 + 1] = -3 + rnd(i + 40) * 12; dustPos[i * 3 + 2] = -10 + rnd(i + 90) * 26; }
  const g = new THREE.BufferGeometry(); g.setAttribute('position', new THREE.BufferAttribute(dustPos, 3));
  const c = document.createElement('canvas'); c.width = c.height = 32; const x = c.getContext('2d');
  const gr = x.createRadialGradient(16, 16, 0, 16, 16, 16); gr.addColorStop(0, 'rgba(255,255,255,1)'); gr.addColorStop(1, 'rgba(255,255,255,0)'); x.fillStyle = gr; x.fillRect(0, 0, 32, 32);
  dust = new THREE.Points(g, new THREE.PointsMaterial({ map: new THREE.CanvasTexture(c), size: 0.07, transparent: true, opacity: 0.5, depthWrite: false, color: 0xffe2b8, blending: THREE.AdditiveBlending }));
  scene.add(dust);
}

// ---------- the cards ----------
const CW = 1.6, CH = 1.2, CD = 0.03;
function roundedRect(w, h, r) {
  const s = new THREE.Shape(), x = -w / 2, y = -h / 2;
  s.moveTo(x + r, y); s.lineTo(x + w - r, y); s.absarc(x + w - r, y + r, r, -Math.PI / 2, 0, false);
  s.lineTo(x + w, y + h - r); s.absarc(x + w - r, y + h - r, r, 0, Math.PI / 2, false);
  s.lineTo(x + r, y + h); s.absarc(x + r, y + h - r, r, Math.PI / 2, Math.PI, false);
  s.lineTo(x, y + r); s.absarc(x + r, y + r, r, Math.PI, Math.PI * 1.5, false); return s;
}
const cardGeo = new THREE.ExtrudeGeometry(roundedRect(CW, CH, 0.07), { depth: CD, bevelEnabled: true, bevelThickness: 0.006, bevelSize: 0.006, bevelSegments: 2, curveSegments: 8 });
cardGeo.translate(0, 0, -CD / 2);
const sideMat = new THREE.MeshStandardMaterial({ color: 0xe6dcc8, roughness: 0.7 });
const aniso = Math.min(8, renderer.capabilities.getMaxAnisotropy());
function faceTexture(canvasEl) {
  const t = new THREE.CanvasTexture(canvasEl); t.colorSpace = THREE.SRGBColorSpace; t.anisotropy = aniso;
  t.repeat.set(1 / CW, 1 / CH); t.offset.set(0.5, 0.5); return t;
}
function faceMaterial(tex) {
  return new THREE.MeshPhysicalMaterial({ map: tex, roughness: 0.6, metalness: 0, clearcoat: 0.3, clearcoatRoughness: 0.4, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.1 });
}
const wireGeo = new THREE.BufferGeometry().setFromPoints([
  new THREE.Vector3(-CW * 0.38, CH / 2, 0), new THREE.Vector3(-CW * 0.38, CH / 2 + 14, 0),
  new THREE.Vector3(CW * 0.38, CH / 2, 0), new THREE.Vector3(CW * 0.38, CH / 2 + 14, 0)
]);

const cards = [];
const Qz = new THREE.Quaternion(), V = new THREE.Vector3(), V2 = new THREE.Vector3(), Q = new THREE.Quaternion(), E = new THREE.Euler();
function buildCards() {
  // three tiers, like an amphitheatre of hanging plates: near and low, far and high
  const tiers = [{ r: 9.5, n: 9, y: -0.7, sc: 1.0 }, { r: 14.5, n: 12, y: 1.0, sc: 1.4 }, { r: 20, n: 16, y: 2.5, sc: 1.85 }];
  let idx = 0;
  tiers.forEach((tr, ti) => {
    const span = 2.5, off = ti % 2 ? 0.5 : 0;
    for (let k = 0; k < tr.n; k++) {
      const tool = TOOLS[(idx * 13 + 5) % TOOLS.length]; // shuffled so categories mix
      const th = ((k + 0.5 + off) / (tr.n + 0.5) - 0.5) * span - off * 0.04;
      const x = CX + tr.r * Math.sin(th), z = CZ - tr.r * Math.cos(th);
      const y = tr.y + Math.sin(k * 1.9 + ti) * 0.8 + (rnd(idx + 3) - 0.5) * 0.7;
      const tex = faceTexture(paintCard(tool, 'plate'));
      const face = faceMaterial(tex);
      const mesh = new THREE.Mesh(cardGeo, [face, sideMat]);
      mesh.castShadow = !phone; mesh.receiveShadow = false;
      const wireMat = new THREE.LineBasicMaterial({ color: 0x8a7d68, transparent: true, opacity: 0.55 });
      const wires = new THREE.LineSegments(wireGeo, wireMat);
      const grp = new THREE.Group(); grp.add(mesh, wires);
      E.set((rnd(idx + 11) - .5) * 0.08, -th + (rnd(idx + 17) - .5) * 0.12, (rnd(idx + 23) - .5) * 0.1, 'YXZ');
      const home = { pos: new THREE.Vector3(x, y, z), quat: new THREE.Quaternion().setFromEuler(E), scale: tr.sc };
      grp.position.copy(home.pos); grp.quaternion.copy(home.quat); grp.scale.setScalar(home.scale);
      scene.add(grp);
      cards.push({ tool, grp, face, wireMat, home, pull: 0, rec: 0, glow: 0, rank: 0, sway: rnd(idx + 5) * 6.28 });
      idx++;
    }
  });
}

// ---------- state driven by the bar ----------
let ranks = new Map(), lastN = 1, chosen = null, bar = null, dock = null, lastTexFile = null;
const maxN = () => (innerWidth / innerHeight < 0.8 ? 2 : 5);
function setTargets(matches) {
  ranks = new Map();
  let list = (matches || []).filter((m, i) => m.score >= matches[0].score * 0.34).map(m => m.tool);
  if (chosen) list = [chosen, ...list.filter(t => t !== chosen)];
  list.slice(0, maxN()).forEach((t, r) => ranks.set(t.i, r));
  if (ranks.size) lastN = ranks.size;
  document.body.classList.toggle('typing', !!(bar && bar.input.value.trim()) || !!chosen);
}

function startDock(file, tool) {
  chosen = tool; setTargets(bar.matches);
  paintFile(file, 'plate').then(({ canvas: cv }) => {
    if (dock && dock.mesh) { scene.remove(dock.mesh); }
    const tex = faceTexture(cv); const face = faceMaterial(tex); face.emissiveIntensity = 0.06;
    const mesh = new THREE.Mesh(cardGeo, [face, sideMat]); mesh.castShadow = !phone; scene.add(mesh);
    dock = { mesh, tool, t: 0, fade: false, s: 0 };
    bar.setNote(`${file.name} is flying into ${tool.name}`);
  });
}
function clearDock() {
  chosen = null; setTargets(bar.matches);
  if (dock) dock.fade = true; bar.setNote('');
}

// ---------- boot ----------
await fontsReady();
buildCards();
bar = createBar({
  mount: document.getElementById('barwrap'),
  onChange: (q, matches, hover) => { if (q.trim() === '' && !bar.file) chosen = null; setTargets(q.trim() || bar.file ? matches : []); },
  onFile: (file, kind) => { const m = bar.matches[0]; startDock(file, m ? m.tool : TOOLS.find(t => t.name === 'Compress image')); },
  onPick: (tool, file) => { chosen = tool; setTargets(bar.matches); if (file) { startDock(file, tool); } else bar.setNote('Drop a file to run ' + tool.name); }
});
addEventListener('keydown', e => { if (e.key === 'Escape') clearDock(); });

// ---------- loop ----------
function resize() {
  const w = innerWidth, h = innerHeight; renderer.setSize(w, h, false); camera.aspect = w / h;
  camera.fov = phone || w / h < 0.8 ? 62 : 50; camera.updateProjectionMatrix();
}
addEventListener('resize', resize); resize();
let px = 0, py = 0, tpx = 0, tpy = 0;
addEventListener('pointermove', e => { tpx = e.clientX / innerWidth * 2 - 1; tpy = e.clientY / innerHeight * 2 - 1; });
const clock = new THREE.Clock(); let T = 0, first = true;
const fwd = new THREE.Vector3(), right = new THREE.Vector3(), up = new THREE.Vector3(), cpos = new THREE.Vector3();
const ease3 = u => (u < .5 ? 4 * u * u * u : 1 - Math.pow(-2 * u + 2, 3) / 2);
const kp = RM ? 14 : 3.0;

function frame() {
  const dt = Math.min(clock.getDelta(), REC ? 0.3 : 0.05); T += dt;
  const amp = RM ? 0 : 1;
  px = damp(px, tpx, 2, dt); py = damp(py, tpy, 2, dt);
  camera.position.set(Math.sin(T * 0.06) * 1.7 * amp + px * 0.5, 0.35 + Math.sin(T * 0.045) * 0.22 * amp - py * 0.25, ZBASE + Math.sin(T * 0.035) * 0.7 * amp);
  camera.lookAt(Math.sin(T * 0.05 + 1) * 2.6 * amp + px * 1.3, 0.8 + Math.sin(T * 0.04) * 0.3 * amp - py * 0.3, -2);
  camera.updateMatrixWorld();
  cpos.copy(camera.position); fwd.set(0, 0, -1).applyQuaternion(camera.quaternion); right.set(1, 0, 0).applyQuaternion(camera.quaternion); up.set(0, 1, 0).applyQuaternion(camera.quaternion);
  const D = phone ? 5.6 : 4.7, SP = phone ? 1.3 : 1.62, YL = phone ? 0.55 : 0.5, anyRank = ranks.size > 0;

  for (const c of cards) {
    const r = ranks.get(c.tool.i);
    const tp = r !== undefined ? 1 : 0, tr = anyRank && r === undefined ? 1 : 0;
    if (r !== undefined) c.rank = r;
    c.pull = damp(c.pull, tp, kp - (r || 0) * 0.15, dt);
    c.rec = damp(c.rec, tr, kp * 0.8, dt);
    const isChosen = chosen === c.tool;
    c.glow = damp(c.glow, isChosen ? (dock && dock.t > 2.4 ? 0.55 : 0.28) : r !== undefined ? 0.1 : 0, 2.5, dt);
    const e = sstep(0, 1, c.pull);
    const sway = RM ? 0 : Math.sin(T * 0.3 + c.sway) * 0.012;
    // home pose, pushed away from the camera if receding
    V.copy(c.home.pos); V2.copy(V).sub(cpos).normalize(); V.addScaledVector(V2, 4.5 * sstep(0, 1, c.rec));
    // pulled pose, in front of the camera
    const n = lastN, off = (c.rank - (n - 1) / 2) * SP;
    const pulled = V2.copy(cpos).addScaledVector(fwd, D + (isChosen ? -0.3 : 0)).addScaledVector(right, phone ? 0 : off).addScaledVector(up, phone ? YL - off * 0.95 : YL);
    c.grp.position.copy(V).lerp(pulled, e);
    E.set(0, 0, off * 0.02);
    Q.copy(camera.quaternion).multiply(Qz.setFromEuler(E));
    c.grp.quaternion.copy(c.home.quat).slerp(Q, e);
    c.grp.rotateZ(sway);
    c.grp.scale.setScalar(lerp(c.home.scale, (phone ? 0.95 : 0.95) * (isChosen ? 1.22 : 1), e) * (1 - 0.05 * c.rec));
    const dim = 1 - 0.74 * sstep(0, 1, c.rec);
    c.face.color.setScalar(dim);
    c.face.emissiveIntensity = 0.1 * dim + c.glow;
    c.wireMat.opacity = 0.55 * (1 - e) * dim;
  }

  if (dock) {
    dock.t += dt; const m = dock.mesh;
    const tc = cards.find(c => c.tool === dock.tool);
    if (dock.fade) { if (!dock.s) dock.s0 = m.scale.x; dock.s += dt; const k = 1 - sstep(0, 0.7, dock.s); m.scale.setScalar(dock.s0 * k); m.position.addScaledVector(up, -dt * 0.4); if (k < 0.02) { scene.remove(m); dock = null; } }
    else if (tc) {
      const ts = tc.grp.scale.x; const tpos = tc.grp.position;
      const u = ease3(clamp(dock.t / 1.9, 0, 1)), d = clamp((dock.t - 1.9) / 1.7, 0, 1);
      const inFront = lerp(0.24, 0.05, sstep(0, 0.4, d)), peek = 0.17 * ts * sstep(0.35, 1, d);
      const end = V.copy(tpos).addScaledVector(fwd, -inFront * ts).addScaledVector(up, -peek);
      const start = V2.copy(cpos).addScaledVector(fwd, 3.3).addScaledVector(up, -1.9);
      const pos = new THREE.Vector3().copy(start).lerp(end, u).addScaledVector(up, Math.sin(Math.PI * u) * 0.55 * (1 - d));
      m.position.copy(pos);
      E.set(0, 0, lerp(-0.3, 0, u)); m.quaternion.copy(camera.quaternion).multiply(Qz.setFromEuler(E));
      m.scale.setScalar(lerp(1.05, 0.97, u) * ts);
      if (dock.t > 3.6 && !dock.done) { dock.done = true; bar.setNote(`${bar.file ? bar.file.name : 'File'} is in ${dock.tool.name}. Nothing leaves this computer.`); }
    }
  }

  if (dust) {
    for (let i = 0; i < dustPos.length; i += 3) { dustPos[i + 1] += dt * 0.05 * (0.4 + rnd(i)); dustPos[i] += Math.sin(T * 0.1 + i) * dt * 0.02; if (dustPos[i + 1] > 9) dustPos[i + 1] = -3; }
    dust.geometry.attributes.position.needsUpdate = true;
  }
  renderer.render(scene, camera);
  if (first) { first = false; document.documentElement.dataset.ready = '1'; }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
