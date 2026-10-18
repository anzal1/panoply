// THE FAN: the Panoply brand mark at world scale. Scroll opens 37 tool cards around the viewer, then closes back to the logo.
import * as THREE from 'three';
import { TOOLS, paintCard, paintFile, fontsReady, reduced, isPhone, clamp, lerp, sstep, damp } from './tools.js';
import { createBar } from './bar.js';

/* ---------- constants ---------- */
const TAU = Math.PI * 2;
const N = TOOLS.length;                    // 37
const LAST = N - 1;                        // 36
const MID = LAST / 2;                      // 18
const CARD_W = 2.2, CARD_H = 3.96;
const CARD_R = CARD_W * 45.552 / 210.241;  // logo corner radius, scaled
const DEPTH = 0.03, BEVEL = 0.004;
const FACE_Z = DEPTH / 2 + BEVEL;          // z of the front cap after centring
const LOGO_A = 20 * Math.PI / 180;
const PIVOT_Y = -1.9;
const R_LOGO = 168 * CARD_W / 210.241 + CARD_H / 2;  // pivot sits 168 logo units below the card bottom
const R_OPEN = 3.4;
const OPEN_ANGLE = 1.9;
const RING_R = 13, RING_Y = 0.4;
const DELTA = TAU / N;
const FLOOR_Y = -2.4;
const Z_STEP = 0.01;                       // stack spacing between neighbouring cards
const LOGO_COL = { first: '#727884', mid: '#2D6CDF', last: '#808692' };
const SIDE_COL = '#D9DCE3';

const phone = isPhone();
const rec = new URLSearchParams(location.search).get('rec') === '1';
const calm = reduced();
const shadowsOn = !phone;

/* ---------- small helpers ---------- */
const wrapPi = a => { a = (a + Math.PI) % TAU; if (a < 0) a += TAU; return a - Math.PI; };
const WHITE = new THREE.Color(1, 1, 1);
const DIMC = new THREE.Color('#B8BCC6');

function roundedRect(w, h, r) {
  const x = -w / 2, y = -h / 2, s = new THREE.Shape();
  s.moveTo(x + r, y);
  s.lineTo(x + w - r, y);
  s.absarc(x + w - r, y + r, r, -Math.PI / 2, 0, false);
  s.lineTo(x + w, y + h - r);
  s.absarc(x + w - r, y + h - r, r, 0, Math.PI / 2, false);
  s.lineTo(x + r, y + h);
  s.absarc(x + r, y + h - r, r, Math.PI / 2, Math.PI, false);
  s.lineTo(x, y + r);
  s.absarc(x + r, y + r, r, Math.PI, Math.PI * 1.5, false);
  return s;
}

/* ---------- state shared with the bar ---------- */
const S = { matched: new Map(), active: false, hover: -1, onFile: null, onPick: null };

const bar = createBar({
  mount: document.getElementById('bar'),
  placeholder: 'Drop a file, or type what you want to do',
  onChange: (query, matches, hoverTool) => {
    S.matched = new Map(matches.map((m, j) => [m.tool.i, 1 - j * 0.1]));
    S.active = matches.length > 0 && ((query || '').trim().length > 0 || !!bar.file);
    S.hover = hoverTool ? hoverTool.i : -1;
  },
  onFile: (f, k) => { if (S.onFile) S.onFile(f, k); },
  onPick: (t, f) => { if (S.onPick) S.onPick(t, f); }
});

/* ---------- main ---------- */
async function main() {
  await fontsReady();

  const canvas = document.getElementById('c');
  const renderer = new THREE.WebGLRenderer({
    canvas, antialias: !phone && !rec, alpha: false, powerPreference: 'high-performance'
  });
  renderer.setPixelRatio(phone ? Math.min(window.devicePixelRatio || 1, 1.5) : Math.min(window.devicePixelRatio || 1, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  renderer.shadowMap.enabled = shadowsOn;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.setClearColor(new THREE.Color('#E9EAEE'), 1);

  const scene = new THREE.Scene();
  scene.fog = new THREE.Fog('#E9EAEE', 55, 150);

  const camera = new THREE.PerspectiveCamera(36, window.innerWidth / window.innerHeight, 0.3, 400);
  camera.position.set(0, 0.4, 15);
  const helper = new THREE.PerspectiveCamera();   // used only to compute lookAt quaternions

  /* ---- cyclorama backdrop: a sphere that follows the camera ---- */
  const bgMat = new THREE.ShaderMaterial({
    side: THREE.BackSide, depthWrite: false, depthTest: false, fog: false,
    uniforms: {
      uTop: { value: new THREE.Color('#E9EAEE') },
      uHorizon: { value: new THREE.Color('#F4F5F7') },
      uBottom: { value: new THREE.Color('#E9EAEE') }
    },
    vertexShader: `
      varying vec3 vDir;
      void main(){
        vDir = normalize(position);
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }`,
    fragmentShader: `
      uniform vec3 uTop; uniform vec3 uHorizon; uniform vec3 uBottom;
      varying vec3 vDir;
      void main(){
        float h = vDir.y;
        float k = exp(-abs(h) * 5.0);
        vec3 base = h > 0.0 ? uTop : uBottom;
        vec3 col = mix(base, uHorizon, k);
        col += (fract(sin(dot(gl_FragCoord.xy, vec2(12.9898, 78.233))) * 43758.5453) - 0.5) / 255.0;
        gl_FragColor = vec4(col, 1.0);
        #include <colorspace_fragment>
      }`
  });
  const bg = new THREE.Mesh(new THREE.SphereGeometry(350, 32, 16), bgMat);
  bg.renderOrder = -10;
  bg.frustumCulled = false;
  scene.add(bg);

  /* ---- floor that only shows shadows ---- */
  const floor = new THREE.Mesh(
    new THREE.PlaneGeometry(600, 600),
    new THREE.ShadowMaterial({ color: 0x2b3350, opacity: 0.2, transparent: true, depthWrite: false })
  );
  floor.rotation.x = -Math.PI / 2;
  floor.position.y = FLOOR_Y;
  floor.receiveShadow = true;
  floor.visible = shadowsOn;
  scene.add(floor);

  /* ---- lights ---- */
  const hemi = new THREE.HemisphereLight(0xffffff, 0xc5c9d3, 1.4);
  scene.add(hemi);
  const key = new THREE.DirectionalLight(0xffffff, 1.7);
  key.position.set(4, 10, 7);
  key.target.position.set(0, 0, 0);
  scene.add(key, key.target);
  if (shadowsOn) {
    key.castShadow = true;
    key.shadow.mapSize.set(2048, 2048);
    const sc = key.shadow.camera;
    sc.left = -20; sc.right = 20; sc.top = 20; sc.bottom = -20; sc.near = 1; sc.far = 40;
    sc.updateProjectionMatrix();
    key.shadow.bias = -0.0004;
    key.shadow.normalBias = 0.02;
    key.shadow.radius = 5;
  }

  /* ---- card geometry and materials ---- */
  const geo = new THREE.ExtrudeGeometry(roundedRect(CARD_W, CARD_H, CARD_R), {
    depth: DEPTH, bevelEnabled: true, bevelThickness: BEVEL, bevelSize: BEVEL, bevelSegments: 2,
    curveSegments: 14, steps: 1
  });
  geo.translate(0, 0, -DEPTH / 2);
  const skinGeo = new THREE.ShapeGeometry(roundedRect(CARD_W, CARD_H, CARD_R), 14);
  const maxAniso = Math.min(8, renderer.capabilities.getMaxAnisotropy());

  function makeTexture(cv) {
    const tex = new THREE.CanvasTexture(cv);
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.anisotropy = maxAniso;
    tex.repeat.set(1 / CARD_W, 1 / CARD_H);
    tex.offset.set(0.5, 0.5);
    tex.needsUpdate = true;
    return tex;
  }
  function makeFace(tex) {
    return new THREE.MeshPhysicalMaterial({
      map: tex, roughness: 0.5, metalness: 0, clearcoat: 0.25, clearcoatRoughness: 0.4,
      emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0
    });
  }
  function makeSide(col) {
    return new THREE.MeshStandardMaterial({ color: col, roughness: 0.6, metalness: 0 });
  }

  /* ---- the 37 cards ---- */
  const cards = [];
  const zFan = [];
  const root = new THREE.Group();
  scene.add(root);
  for (let i = 0; i < N; i++) {
    const tool = TOOLS[i];
    const tex = makeTexture(paintCard(tool, 'fan'));
    const face = makeFace(tex);
    const side = makeSide(SIDE_COL);
    const mesh = new THREE.Mesh(geo, [face, side]);
    mesh.castShadow = shadowsOn;
    const group = new THREE.Group();
    group.add(mesh);

    let skin = null, logoCol = null;
    if (i === 0 || i === MID || i === LAST) {
      logoCol = new THREE.Color(i === 0 ? LOGO_COL.first : i === LAST ? LOGO_COL.last : LOGO_COL.mid);
      const skinMat = new THREE.MeshBasicMaterial({
        color: logoCol, transparent: true, opacity: 1, depthWrite: false, toneMapped: false,
        polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -1
      });
      skin = new THREE.Mesh(skinGeo, skinMat);
      skin.position.z = FACE_Z + 0.006;
      group.add(skin);
    }
    root.add(group);
    // stack order: card 18 highest, card 36 slightly above card 0 so the logo overlap reads right
    zFan.push(Z_STEP * (MID - Math.abs(i - MID)) + (i > MID ? Z_STEP * 0.5 : 0));
    cards.push({ group, face, side, tex, skin, logoCol, glow: 0, dim: 0, lift: 0 });
  }

  /* ---------- camera framing per aspect ---------- */
  const FOV_FAR = phone ? 40 : 36;
  const FOV_RING = phone ? 52 : 46;
  const view = { dsLogo: 1, dsFan: 1 };
  function layout() {
    const w = window.innerWidth, h = window.innerHeight;
    renderer.setSize(w, h);
    camera.aspect = w / h;
    const w15 = 2 * 15 * Math.tan(THREE.MathUtils.degToRad(FOV_FAR / 2)) * camera.aspect;  // visible width at distance 15
    view.dsLogo = clamp(7.8 / w15, 1, 2.4);
    view.dsFan = clamp(Math.max(12.6 / w15, 1.08), view.dsLogo, 3.1);
    camera.updateProjectionMatrix();
  }
  layout();
  window.addEventListener('resize', layout);

  /* ---------- scroll ---------- */
  const readScroll = () => {
    const max = Math.max(1, document.documentElement.scrollHeight - window.innerHeight);
    return clamp(window.scrollY / max, 0, 1);
  };
  let pt = readScroll();
  let ps = pt;

  window.__setProgress = (p, instant = true) => {
    const max = Math.max(1, document.documentElement.scrollHeight - window.innerHeight);
    window.scrollTo(0, clamp(p, 0, 1) * max);
    pt = readScroll();
    if (instant) ps = pt;
  };

  /* ---------- reusable temporaries ---------- */
  const zAxis = new THREE.Vector3(0, 0, 1);
  const yAxis = new THREE.Vector3(0, 1, 0);
  const qF = new THREE.Quaternion(), qR = new THREE.Quaternion();
  const qFar = new THREE.Quaternion(), qRing = new THREE.Quaternion(), qTmp = new THREE.Quaternion();
  const eul = new THREE.Euler();
  const farPos = new THREE.Vector3(), ringPos = new THREE.Vector3(0, RING_Y, 0);
  const vTmp = new THREE.Vector3();

  /* ---------- the pose of every card plus the camera ---------- */
  function pose(p, t) {
    // logo closedness, fan openness, fan -> ring morph
    const c = clamp(1 - sstep(0.10, 0.17, p) + sstep(0.93, 1.0, p), 0, 1);
    const openAmt = sstep(0.10, 0.30, p) * (1 - sstep(0.88, 1.0, p));
    const openness = lerp(0.15, 1, openAmt);
    const r = sstep(0.28, 0.46, p) * (1 - sstep(0.80, 0.92, p));
    const psi = -(LAST * DELTA) * sstep(0.46, 0.80, p) - DELTA * sstep(0.80, 0.92, p);
    const Rf = lerp(R_OPEN, R_LOGO, c);

    for (let i = 0; i < N; i++) {
      const ci = cards[i];
      const g = ci.group;
      const aOpen = lerp(-OPEN_ANGLE, OPEN_ANGLE, i / LAST) * openness;
      const aLogo = i === 0 ? -LOGO_A : i === LAST ? LOGO_A : 0;
      const a = lerp(aOpen, aLogo, c);
      const fx = Rf * Math.sin(a);
      const fy = PIVOT_Y + Rf * Math.cos(a);
      const fz = zFan[i];
      qF.setFromAxisAngle(zAxis, -a);

      // ring pose: faces the centre, leans like a hand fan, hero card near the view direction
      const phi = -(i - MID) * DELTA;
      const d = wrapPi(phi - psi);
      const wgt = Math.exp(-Math.pow(d / (0.85 * DELTA), 2));
      const lean = clamp(d * 1.6, -0.9, 0.9);
      const roll = clamp(d * 0.5, -0.7, 0.7);
      const rr = RING_R - 2 * wgt;
      const rx = -rr * Math.sin(phi), ry = RING_Y, rz = -rr * Math.cos(phi);
      eul.set(0, phi + lean, roll, 'YXZ');
      qR.setFromEuler(eul);

      // morph with a ripple: centre cards lead going out and leave last coming back
      const dm = Math.abs(i - MID) / MID;
      const tm = sstep(0, 1, clamp(r * 1.35 - 0.35 * dm, 0, 1));

      g.position.set(lerp(fx, rx, tm), lerp(fy, ry, tm), lerp(fz, rz, tm));
      g.quaternion.copy(qF).slerp(qR, tm);
      g.scale.setScalar(lerp(1, 1 + 0.3 * wgt, tm));

      if (ci.skin) {
        ci.skin.material.opacity = c;
        ci.skin.visible = c > 0.003;
        ci.side.color.set(SIDE_COL).lerp(ci.logoCol, c);
      }
    }

    // camera: far framing blended into the ring centre, camera leads the cards
    const rc = 1 - Math.pow(1 - r, 2.5);
    const dolly = sstep(0.10, 0.28, p) * (1 - sstep(0.80, 0.92, p));
    const ds = lerp(view.dsLogo, view.dsFan, openAmt);
    const farZ = 15 * ds * (1 - 0.08 * dolly);
    const drift = calm ? 0 : 1;
    farPos.set(Math.sin(t * 0.21) * 0.28 * ds * drift, 0.4 + Math.sin(t * 0.17 + 1) * 0.07 * drift, farZ);
    const tY = lerp(2.0, -0.1, openAmt);
    helper.position.copy(farPos);
    helper.lookAt(0, tY, 0);
    qFar.copy(helper.quaternion);
    qRing.setFromEuler(eul.set(0, psi, 0, 'YXZ'));

    camera.position.lerpVectors(farPos, ringPos, rc);
    camera.quaternion.copy(qFar).slerp(qRing, rc);
    camera.fov = lerp(FOV_FAR, FOV_RING, rc);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
    bg.position.copy(camera.position);
  }

  /* ---------- bar highlight ---------- */
  function updateHighlights(dt) {
    for (let i = 0; i < N; i++) {
      const ci = cards[i];
      const hit = S.matched.get(i) || 0;
      const glowT = S.active ? Math.max(hit, S.hover === i ? 1 : 0) : 0;
      const dimT = S.active && hit === 0 && S.hover !== i ? 1 : 0;
      ci.glow = damp(ci.glow, glowT, 8, dt);
      ci.dim = damp(ci.dim, dimT, 8, dt);
      ci.lift = Math.max(0, ci.lift - dt / 1.0);
      ci.face.color.copy(WHITE).lerp(DIMC, 0.6 * ci.dim);
      ci.face.emissiveIntensity = 0.35 * ci.glow + 0.55 * sstep(0, 1, ci.lift);
    }
  }

  /* ---------- dropped file card ---------- */
  let job = null;
  function endJob() {
    if (!job) return;
    scene.remove(job.mesh);
    job.tex.dispose();
    job.mats.forEach(m => m.dispose());
    job = null;
  }
  S.onFile = async (file, kind) => {
    bar.setNote('Nothing leaves this computer.');
    const m = bar.matches[0];
    let idx = m ? m.tool.i : TOOLS.findIndex(t => t.cat === kind);
    if (idx < 0) idx = 0;
    let painted;
    try { painted = await paintFile(file, 'fan'); } catch (e) { return; }
    endJob();
    const tex = makeTexture(painted.canvas);
    const face = makeFace(tex), side = makeSide(SIDE_COL);
    const mesh = new THREE.Mesh(geo, [face, side]);
    mesh.frustumCulled = false;
    scene.add(mesh);
    job = { mesh, tex, mats: [face, side], t: 0, idx };
  };
  S.onPick = (tool) => {
    cards[tool.i].lift = 1;
    bar.setNote('Nothing leaves this computer.');
  };

  function updateJob(dt) {
    if (!job) return;
    job.t += dt;
    const t = job.t, T1 = 0.9, T2 = 1.4, T3 = 1.1;
    const D = 7;
    const hh = D * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2));   // half visible height at distance D
    const sf = 0.42 * 2 * hh / CARD_H;                                   // card fills about 42% of the viewport height
    const yHide = -(hh + CARD_H * sf * 0.6);
    const yShow = hh * 0.1;
    const rise = sstep(0, 1, t / T1);
    const y = t < T1 ? lerp(yHide, yShow, rise) : yShow + Math.sin((t - T1) * 2) * hh * 0.015;
    const mesh = job.mesh;
    mesh.position.set(0, y, -D).applyMatrix4(camera.matrixWorld);
    qTmp.setFromAxisAngle(yAxis, Math.sin(t * 1.1) * 0.05 * rise);
    mesh.quaternion.copy(camera.quaternion).multiply(qTmp);
    mesh.scale.setScalar(sf);

    if (t > T1 + T2) {
      const u = sstep(0, 1, (t - T1 - T2) / T3);
      const tg = cards[job.idx].group;
      vTmp.set(0, 0, -0.06).applyQuaternion(tg.quaternion).multiplyScalar(tg.scale.x).add(tg.position);
      mesh.position.lerp(vTmp, u);
      mesh.quaternion.slerp(tg.quaternion, u);
      mesh.scale.setScalar(lerp(sf, tg.scale.x, u));
      if (t >= T1 + T2 + T3) {
        cards[job.idx].lift = 1;
        endJob();
      }
    }
  }

  /* ---------- DOM text ---------- */
  const h1 = document.getElementById('h1');
  const h2 = document.getElementById('h2');
  const hint = document.getElementById('hint');
  const lastOp = { h1: -1, h2: -1, hint: -1 };
  function setFade(el, key, o, rise) {
    if (Math.abs(o - lastOp[key]) < 0.002 && !(o === 0 && lastOp[key] !== 0) && !(o === 1 && lastOp[key] !== 1)) return;
    lastOp[key] = o;
    el.style.opacity = o.toFixed(3);
    el.style.transform = 'translateY(' + ((1 - o) * rise).toFixed(1) + 'px)';
    el.style.visibility = o < 0.01 ? 'hidden' : 'visible';
  }
  function updateText(p) {
    setFade(h1, 'h1', 1 - sstep(0.05, 0.12, p), -18);
    setFade(h2, 'h2', sstep(0.93, 0.98, p), 18);
    if (hint) setFade(hint, 'hint', 1 - sstep(0.01, 0.05, p), 0);
  }

  /* ---------- render loop ---------- */
  const clock = new THREE.Clock();
  let readyFlag = false;
  function frame() {
    requestAnimationFrame(frame);
    const dt = Math.min(clock.getDelta(), 0.05);
    const t = clock.elapsedTime;
    pt = readScroll();
    ps = damp(ps, pt, calm ? 18 : 4.4, dt);
    if (Math.abs(ps - pt) < 1e-4) ps = pt;

    pose(ps, t);
    updateHighlights(dt);
    updateJob(dt);
    updateText(ps);

    renderer.render(scene, camera);
    if (!readyFlag) {
      readyFlag = true;
      document.documentElement.dataset.ready = '1';
    }
  }
  requestAnimationFrame(frame);
}

main().catch(err => {
  console.error('THE FAN failed to start', err);
  document.documentElement.dataset.error = '1';
});
