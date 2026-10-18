// NEVER LEAVES: the visitor's computer as an infinite glowing interior with a visible horizon wall.
// Plain ES module, three@0.170 core only, no build step.
import * as THREE from 'three';
import { TOOLS, paintCard, paintFile, fontsReady, reduced, isPhone, clamp, lerp, sstep, damp, rnd, search } from './tools.js';
import { createBar } from './bar.js';

const PHONE = isPhone();
const REDUCED = reduced();
const REC = new URLSearchParams(location.search).get('rec') === '1';
const MIRROR = !PHONE;

const WALL_R = 95, RING_R = 66, INNER_R = 14, BAND_LO = 11.2, BAND_HI = 18.6, SPOKE_R = 48;
const BG = 0x06080c, WARM = 0xFFEFD6, RAIL = 0x9DB8F0, SOFTBLUE = 0x4F86F0;
const MESSAGE = 'Nothing leaves this computer.';

// ===PURE-START=== (no THREE, no DOM in this block: it is unit-tested in node)
const GOLDEN = 2.399963229728653;

function mulberry(a){
  return function(){
    a |= 0; a = a + 0x6D2B79F5 | 0;
    let t = Math.imul(a ^ a >>> 15, 1 | a);
    t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}

// Calm phyllotaxis, radius 6 to 42, with a clear lane around the camera path.
function layoutStations(n){
  const out = [];
  for(let i = 0; i < n; i++){
    const t = n > 1 ? i / (n - 1) : 0;
    let r = 6 + 36 * Math.pow(t, .72);
    if(r < 14.5) r = 6 + (r - 6) * (BAND_LO - 6) / (14.5 - 6);
    else r = BAND_HI + (r - 14.5) * (42 - BAND_HI) / (42 - 14.5);
    const a = i * GOLDEN + .7;
    const x = Math.cos(a) * r, z = Math.sin(a) * r;
    const d = Math.hypot(x, z) || 1;
    out.push({ i, r, a, x, z, fx: -x / d, fz: -z / d, yaw: Math.atan2(-x / d, -z / d) });
  }
  return out;
}

function segDist(px, pz, ax, az, bx, bz){
  const dx = bx - ax, dz = bz - az, l2 = dx * dx + dz * dz;
  let t = l2 > 0 ? ((px - ax) * dx + (pz - az) * dz) / l2 : 0;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + dx * t), pz - (az + dz * t));
}
function segCross(a, b, c, d){
  const o = (p, q, r) => (q.x - p.x) * (r.z - p.z) - (q.z - p.z) * (r.x - p.x);
  const d1 = o(a, b, c), d2 = o(a, b, d), d3 = o(c, d, a), d4 = o(c, d, b);
  return ((d1 > 1e-9 && d2 < -1e-9) || (d1 < -1e-9 && d2 > 1e-9)) && ((d3 > 1e-9 && d4 < -1e-9) || (d3 < -1e-9 && d4 > 1e-9));
}

// Rail graph: forced rings and spokes, then a minimum spanning tree over stations, then a few extras.
function buildGraph(stations){
  const nodes = [], edges = [];
  const add = (x, z, kind, own = -1) => { nodes.push({ x, z, kind, own }); return nodes.length - 1; };
  const front = stations.map(s => add(s.x + s.fx * 1.3, s.z + s.fz * 1.3, 's', s.i));
  const R1N = 48, ring1 = [];
  for(let k = 0; k < R1N; k++){ const a = k / R1N * Math.PI * 2; ring1.push(add(Math.cos(a) * INNER_R, Math.sin(a) * INNER_R, 'g')); }
  const R2N = 72, ring2 = [];
  for(let k = 0; k < R2N; k++){ const a = k / R2N * Math.PI * 2; ring2.push(add(Math.cos(a) * RING_R, Math.sin(a) * RING_R, 'r')); }
  const NS = 6, spokeAt = [], tips = [];
  for(let k = 0; k < NS; k++){
    const idx = (k * (R2N / NS) + 5) % R2N; spokeAt.push(idx);
    const a = idx / R2N * Math.PI * 2; tips.push(add(Math.cos(a) * SPOKE_R, Math.sin(a) * SPOKE_R, 't'));
  }
  const par = nodes.map((_, i) => i);
  const find = x => { while(par[x] !== x){ par[x] = par[par[x]]; x = par[x]; } return x; };
  const key = (a, b) => a < b ? a * 10000 + b : b * 10000 + a;
  const adjSet = new Set();
  const addEdge = (a, b, type, w) => { edges.push({ a, b, type, w }); adjSet.add(key(a, b)); par[find(a)] = find(b); };
  for(let k = 0; k < R1N; k++) addEdge(ring1[k], ring1[(k + 1) % R1N], 'g', .3);
  for(let k = 0; k < R2N; k++) addEdge(ring2[k], ring2[(k + 1) % R2N], 'r', .5);
  for(let k = 0; k < NS; k++) addEdge(ring2[spokeAt[k]], tips[k], 'r', .5);

  function valid(a, b){
    const A = nodes[a], B = nodes[b];
    if(Math.hypot(A.x - B.x, A.z - B.z) > 34) return false;
    for(const s of stations){
      const own = A.own === s.i || B.own === s.i;
      if(segDist(s.x, s.z, A.x, A.z, B.x, B.z) < (own ? 1.15 : 2.3)) return false;
    }
    for(const e of edges){
      if(e.a === a || e.a === b || e.b === a || e.b === b) continue;
      if(segCross(A, B, nodes[e.a], nodes[e.b])) return false;
    }
    return true;
  }
  const pool = [...front, ...ring1, ...tips];
  const cand = [];
  for(let x = 0; x < pool.length; x++) for(let y = x + 1; y < pool.length; y++){
    const a = pool[x], b = pool[y], ka = nodes[a].kind, kb = nodes[b].kind;
    if(ka === 'g' && kb === 'g') continue;
    if(ka === 't' && kb === 't') continue;
    cand.push([a, b, Math.hypot(nodes[a].x - nodes[b].x, nodes[a].z - nodes[b].z)]);
  }
  cand.sort((p, q) => p[2] - q[2]);
  for(const [a, b] of cand){
    if(find(a) === find(b)) continue;
    if(!valid(a, b)) continue;
    addEdge(a, b, 's', .36);
  }
  // Safety net: if anything is still detached, join it by the nearest pair, ignoring clearance.
  for(let guard = 0; guard < 60; guard++){
    const roots = new Set(nodes.map((_, i) => find(i)));
    if(roots.size <= 1) break;
    const main = find(front[0]);
    let best = null;
    for(const [a, b, l] of cand){ if(find(a) !== find(b) && (find(a) === main || find(b) === main) && (!best || l < best[2])) best = [a, b, l]; }
    if(!best) break;
    addEdge(best[0], best[1], 's', .36);
  }
  let extras = 0;
  for(const s of stations){
    if(extras >= 18) break;
    if(rnd(s.i + 40) > .5) continue;
    const a = front[s.i];
    const list = pool.filter(b => b !== a && !adjSet.has(key(a, b)) && nodes[b].kind !== 't')
      .map(b => [b, Math.hypot(nodes[a].x - nodes[b].x, nodes[a].z - nodes[b].z)]).sort((p, q) => p[1] - q[1]);
    for(const [b, l] of list){
      if(l > 16) break;
      if(valid(a, b)){ addEdge(a, b, 's', .36); extras++; break; }
    }
  }
  const adj = nodes.map(() => []);
  for(const e of edges){
    const w = Math.hypot(nodes[e.a].x - nodes[e.b].x, nodes[e.a].z - nodes[e.b].z);
    adj[e.a].push([e.b, w]); adj[e.b].push([e.a, w]);
  }
  return { nodes, edges, adj, front, ring1, ring2, spokeAt, tips };
}

function makeRouter(G){
  const V = G.nodes.length, cache = new Map();
  function run(src){
    if(cache.has(src)) return cache.get(src);
    const dist = new Float64Array(V).fill(Infinity), prev = new Int32Array(V).fill(-1), done = new Uint8Array(V);
    dist[src] = 0;
    for(;;){
      let u = -1, b = Infinity;
      for(let i = 0; i < V; i++) if(!done[i] && dist[i] < b){ b = dist[i]; u = i; }
      if(u < 0) break;
      done[u] = 1;
      for(const [v, w] of G.adj[u]) if(dist[u] + w < dist[v]){ dist[v] = dist[u] + w; prev[v] = u; }
    }
    const r = { dist, prev }; cache.set(src, r); return r;
  }
  function path(a, b){
    if(a === b) return [a];
    const { dist, prev } = run(a);
    if(!isFinite(dist[b])) return [a, b];
    const out = [b]; let c = b;
    while(c !== a){ c = prev[c]; out.push(c); }
    return out.reverse();
  }
  return { path, dist: (a, b) => run(a).dist[b] };
}

// Camera vantage in front of a station with clear space around it.
function vantageFor(s, stations, dists){
  const base = Math.atan2(s.fz, s.fx);
  for(const d of dists) for(const o of [0, .4, -.4, .8, -.8, 1.2, -1.2]){
    const x = s.x + Math.cos(base + o) * d, z = s.z + Math.sin(base + o) * d;
    if(Math.hypot(x, z) > WALL_R - 20) continue;
    let ok = true;
    for(const t of stations) if(t.i !== s.i && Math.hypot(x - t.x, z - t.z) < 2.6){ ok = false; break; }
    if(ok) return { x, z };
  }
  return { x: s.x + s.fx * dists[0], z: s.z + s.fz * dists[0] };
}

// Polyline helpers on plain {x,y,z} points.
function mkPath(pts){
  const cum = [0];
  for(let i = 1; i < pts.length; i++) cum.push(cum[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].z - pts[i - 1].z));
  return { pts, cum, L: cum[cum.length - 1] };
}
function sample(pts, cum, s, pos, dir){
  let i = 1;
  while(i < cum.length - 1 && cum[i] < s) i++;
  const a = pts[i - 1], b = pts[i];
  const segL = (cum[i] - cum[i - 1]) || 1;
  const t = clamp((s - cum[i - 1]) / segL, 0, 1);
  pos.set(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);
  if(dir) dir.set(b.x - a.x, 0, b.z - a.z).normalize();
}
function lerpAngle(a, b, t){
  const d = ((b - a + Math.PI) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2) - Math.PI;
  return a + d * t;
}
// ===PURE-END===

// ---------- shaders ----------
const WALL_VS = `
varying vec3 vW;
#include <fog_pars_vertex>
void main(){
  vec4 wp = modelMatrix * vec4(position, 1.0);
  vW = wp.xyz;
  vec4 mvPosition = viewMatrix * wp;
  gl_Position = projectionMatrix * mvPosition;
  #include <fog_vertex>
}`;
const WALL_FS = `
uniform vec3 uBase;
uniform vec3 uGlow;
uniform vec3 uLine;
uniform float uTime;
varying vec3 vW;
#include <fog_pars_fragment>
void main(){
  float h = abs(vW.y);
  float ang = atan(vW.z, vW.x);
  float ph = ang * 0.15915494 * 288.0;
  float c = abs(fract(ph) - 0.5);
  float rib = 1.0 - smoothstep(0.10, 0.46, c);
  float groove = smoothstep(0.40, 0.50, c);
  float band = exp(-h * 0.17);
  float halo = exp(-h * 0.045) * 0.22;
  float sway = 0.90 + 0.10 * sin(ang * 3.0 + uTime * 0.07) + 0.05 * sin(ang * 7.0 - uTime * 0.05);
  vec3 col = uBase;
  col += uGlow * (band * (0.30 + 0.70 * rib) * (1.0 - 0.55 * groove) + halo * (0.6 + 0.4 * rib)) * sway;
  float line = 1.0 - smoothstep(0.05, 0.17, h);
  float lineHalo = exp(-h * 9.0) * 0.55;
  col += uLine * (line * 1.6 + lineHalo);
  gl_FragColor = vec4(col, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
  #include <fog_fragment>
}`;

// ---------- canvas textures ----------
function mkCanvas(w, h){ const c = document.createElement('canvas'); c.width = w; c.height = h; return c; }
function tex(canvas, aniso = 1){
  const t = new THREE.CanvasTexture(canvas);
  t.colorSpace = THREE.SRGBColorSpace; t.anisotropy = aniso; return t;
}
function dotCanvas(){
  const c = mkCanvas(128, 128), x = c.getContext('2d');
  const g = x.createRadialGradient(64, 64, 0, 64, 64, 64);
  g.addColorStop(0, 'rgba(255,255,255,1)'); g.addColorStop(.25, 'rgba(255,255,255,.55)');
  g.addColorStop(.55, 'rgba(255,255,255,.14)'); g.addColorStop(1, 'rgba(255,255,255,0)');
  x.fillStyle = g; x.fillRect(0, 0, 128, 128); return c;
}
function ringCanvas(){
  const c = mkCanvas(256, 256), x = c.getContext('2d');
  const g = x.createRadialGradient(128, 128, 0, 128, 128, 128);
  g.addColorStop(0, 'rgba(255,255,255,.24)'); g.addColorStop(.5, 'rgba(255,255,255,.10)');
  g.addColorStop(.70, 'rgba(255,255,255,.06)'); g.addColorStop(.77, 'rgba(255,255,255,.55)');
  g.addColorStop(.80, 'rgba(255,255,255,.95)'); g.addColorStop(.83, 'rgba(255,255,255,.55)');
  g.addColorStop(.90, 'rgba(255,255,255,.10)'); g.addColorStop(1, 'rgba(255,255,255,0)');
  x.fillStyle = g; x.fillRect(0, 0, 256, 256); return c;
}
function railCanvas(){
  const c = mkCanvas(4, 64), x = c.getContext('2d');
  for(let y = 0; y < 64; y++){
    const v = (y + .5) / 64, d = Math.abs(v - .5) * 2;
    const a = Math.min(1, Math.exp(-d * d * 8) * .55 + Math.exp(-d * d * 70) * .55);
    x.fillStyle = 'rgba(255,255,255,' + a.toFixed(4) + ')'; x.fillRect(0, y, 4, 1);
  }
  return c;
}
function floorCanvas(){
  const S = 1024, c = mkCanvas(S, S), x = c.getContext('2d');
  const g = x.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
  g.addColorStop(0, '#10161f'); g.addColorStop(.3, '#0b1017'); g.addColorStop(.65, '#07090e');
  g.addColorStop(.9, '#0a0f18'); g.addColorStop(1, '#101a2a');
  x.fillStyle = g; x.fillRect(0, 0, S, S);
  x.strokeStyle = 'rgba(157,184,240,.05)'; x.lineWidth = 1.4;
  for(let u = 12; u < 94; u += 12){ x.beginPath(); x.arc(S / 2, S / 2, u / 94.9 * (S / 2), 0, Math.PI * 2); x.stroke(); }
  return c;
}

function ribbonGeometry(segs, y){
  const n = segs.length;
  const pos = new Float32Array(n * 12), uv = new Float32Array(n * 8), idx = new Uint32Array(n * 6);
  segs.forEach((s, k) => {
    const dx = s.bx - s.ax, dz = s.bz - s.az, l = Math.hypot(dx, dz) || 1;
    const nx = -dz / l * s.w / 2, nz = dx / l * s.w / 2;
    const p = k * 12;
    pos[p] = s.ax + nx; pos[p + 1] = y; pos[p + 2] = s.az + nz;
    pos[p + 3] = s.ax - nx; pos[p + 4] = y; pos[p + 5] = s.az - nz;
    pos[p + 6] = s.bx + nx; pos[p + 7] = y; pos[p + 8] = s.bz + nz;
    pos[p + 9] = s.bx - nx; pos[p + 10] = y; pos[p + 11] = s.bz - nz;
    const q = k * 8;
    uv[q] = .5; uv[q + 1] = 0; uv[q + 2] = .5; uv[q + 3] = 1; uv[q + 4] = .5; uv[q + 5] = 0; uv[q + 6] = .5; uv[q + 7] = 1;
    const v = k * 4, i = k * 6;
    idx[i] = v; idx[i + 1] = v + 1; idx[i + 2] = v + 2; idx[i + 3] = v + 1; idx[i + 4] = v + 3; idx[i + 5] = v + 2;
  });
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  g.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
  g.setIndex(new THREE.BufferAttribute(idx, 1));
  g.computeBoundingSphere();
  return g;
}

async function main(){
  await fontsReady();
  const canvas = document.getElementById('gl');
  let renderer;
  try{
    renderer = new THREE.WebGLRenderer({ canvas, antialias: !REC, powerPreference: 'high-performance' });
  }catch(err){
    console.error(err);
    document.documentElement.dataset.glerror = '1';
    document.documentElement.dataset.ready = '1';
    return;
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, PHONE ? 1.5 : 2));
  renderer.setClearColor(BG, 1);
  renderer.autoClear = false;
  const maxAniso = renderer.capabilities.getMaxAnisotropy ? renderer.capabilities.getMaxAnisotropy() : 1;
  const aniso = Math.min(8, maxAniso);

  const scene = new THREE.Scene();
  scene.fog = new THREE.FogExp2(BG, .0055);
  const camera = new THREE.PerspectiveCamera(52, 1, .1, 400);
  const world = new THREE.Group();
  scene.add(world);

  const eyeH = PHONE ? 2.0 : 1.7;
  const camR = PHONE ? 15.5 : INNER_R;

  // ----- horizon wall (the visible wall nothing crosses) -----
  const wallMat = new THREE.ShaderMaterial({
    side: THREE.BackSide, fog: true,
    uniforms: THREE.UniformsUtils.merge([THREE.UniformsLib.fog, {
      uTime: { value: 0 },
      uBase: { value: new THREE.Color(BG) },
      uGlow: { value: new THREE.Color(RAIL).multiplyScalar(.30) },
      uLine: { value: new THREE.Color(WARM).multiplyScalar(.9) }
    }]),
    vertexShader: WALL_VS, fragmentShader: WALL_FS
  });
  const wall = new THREE.Mesh(new THREE.CylinderGeometry(WALL_R, WALL_R, 90, 160, 1, true), wallMat);
  wall.frustumCulled = false;
  scene.add(wall);

  // ----- floor: dark glossy plane, reflection comes from the mirrored pass beneath -----
  const floorGeo = new THREE.CircleGeometry(WALL_R - .1, 96);
  floorGeo.rotateX(-Math.PI / 2);
  const floorMat = new THREE.MeshBasicMaterial({ map: tex(floorCanvas(), aniso), transparent: true, opacity: .8, depthWrite: false });
  const floor = new THREE.Mesh(floorGeo, floorMat);
  floor.renderOrder = -10; floor.frustumCulled = false;
  scene.add(floor);

  // ----- shared textures and materials -----
  const dotTex = tex(dotCanvas()), ringTex = tex(ringCanvas(), aniso);
  const railTex = tex(railCanvas(), aniso);
  const warm = new THREE.Color(WARM);
  const glowMat = new THREE.SpriteMaterial({ map: dotTex, color: WARM, transparent: true, opacity: .75, blending: THREE.AdditiveBlending, depthWrite: false });

  // ----- stations -----
  const N = TOOLS.length;
  const stations = layoutStations(N);
  const bodyGeo = new THREE.BoxGeometry(1.5, 2.0625, .07); bodyGeo.translate(0, .14 + 1.03125, 0);
  const plinthGeo = new THREE.BoxGeometry(1.9, .14, .7); plinthGeo.translate(0, .07, 0);
  const bodyIM = new THREE.InstancedMesh(bodyGeo, new THREE.MeshBasicMaterial({ color: 0x222c3b }), N);
  const plinthIM = new THREE.InstancedMesh(plinthGeo, new THREE.MeshBasicMaterial({ color: 0x151c27 }), N);
  bodyIM.frustumCulled = false; plinthIM.frustumCulled = false;
  const dummy = new THREE.Object3D();
  const faceGeo = new THREE.PlaneGeometry(1.5, 2.0625);
  const ringGeo = new THREE.PlaneGeometry(1, 1); ringGeo.rotateX(-Math.PI / 2);
  stations.forEach(s => {
    dummy.position.set(s.x, 0, s.z); dummy.rotation.set(0, s.yaw, 0); dummy.scale.set(1, 1, 1); dummy.updateMatrix();
    bodyIM.setMatrixAt(s.i, dummy.matrix); plinthIM.setMatrixAt(s.i, dummy.matrix);
    s.faceMat = new THREE.MeshBasicMaterial({ map: tex(paintCard(TOOLS[s.i], 'glass'), aniso) });
    s.face = new THREE.Mesh(faceGeo, s.faceMat);
    s.face.position.set(s.x + s.fx * .037, .14 + 1.03125, s.z + s.fz * .037); s.face.rotation.y = s.yaw;
    s.ringMat = new THREE.MeshBasicMaterial({ map: ringTex, color: WARM, transparent: true, opacity: .5, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });
    s.ring = new THREE.Mesh(ringGeo, s.ringMat);
    s.ring.position.set(s.x + s.fx * .2, .03, s.z + s.fz * .2); s.ring.scale.set(5.2, 1, 5.2);
    s.haloMat = new THREE.SpriteMaterial({ map: dotTex, color: WARM, transparent: true, opacity: .14, blending: THREE.AdditiveBlending, depthWrite: false });
    s.halo = new THREE.Sprite(s.haloMat);
    s.halo.position.set(s.x - s.fx * .15, 1.2, s.z - s.fz * .15); s.halo.scale.set(4.6, 5.2, 1);
    s.lit = 0; s.dim = 0; s.dock = 0; s.dockT = 0;
    world.add(s.ring, s.halo, s.face);
  });
  bodyIM.instanceMatrix.needsUpdate = true; plinthIM.instanceMatrix.needsUpdate = true;
  world.add(bodyIM, plinthIM);

  const vantDist = PHONE ? [9.5, 11, 8] : [6.8, 8.2, 5.6];
  const vantage = stations.map(s => vantageFor(s, stations, vantDist));

  // ----- rails -----
  const G = buildGraph(stations);
  const router = makeRouter(G);
  const segs = G.edges.map(e => ({ ax: G.nodes[e.a].x, az: G.nodes[e.a].z, bx: G.nodes[e.b].x, bz: G.nodes[e.b].z, w: e.w }));
  const railMat = new THREE.MeshBasicMaterial({ map: railTex, color: RAIL, transparent: true, opacity: .38, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });
  const rails = new THREE.Mesh(ribbonGeometry(segs, .02), railMat);
  rails.frustumCulled = false;
  world.add(rails);

  // ----- packets: small physical file cards on light rails -----
  const rand = mulberry(7);
  const cardGeo = new THREE.BoxGeometry(.28, .36, .018);
  const catTint = { pdf: 0xD9544F, image: 0x8E5BC4, media: 0xE08A2E, id: 0xD9A92C, files: 0x2D6CDF };
  const packMats = Object.keys(catTint).map(k => new THREE.MeshBasicMaterial({ color: new THREE.Color(WARM).lerp(new THREE.Color(catTint[k]), .16) }));
  const nPk = PHONE ? 10 : 24, nRing = PHONE ? 3 : 7;
  const near = stations.map(s => stations.filter(t => t.i !== s.i).sort((a, b) => Math.hypot(a.x - s.x, a.z - s.z) - Math.hypot(b.x - s.x, b.z - s.z)).slice(0, 10).map(t => t.i));
  const toPts = ids => ids.map(id => ({ x: G.nodes[id].x, y: 0, z: G.nodes[id].z }));
  function arcIds(k){
    const R2N = G.ring2.length, a = G.spokeAt[k], b = G.spokeAt[(k + 1) % G.spokeAt.length], out = [];
    for(let j = a; ; j = (j + 1) % R2N){ out.push(G.ring2[j]); if(j === b) break; }
    return out;
  }
  const packets = [];
  for(let i = 0; i < nPk; i++){
    const ringP = i < nRing;
    const mesh = new THREE.Mesh(cardGeo, packMats[i % packMats.length]);
    const glow = new THREE.Sprite(glowMat);
    glow.position.set(0, -.1, 0);
    mesh.add(glow);
    const p = { mesh, glow, ring: ringP, v: ringP ? 3.0 : 1.6, pause: 0, phase: rand() * 6.28, base: ringP ? 2.1 : 1, node: 0, dest: 0, arc: i % G.spokeAt.length, trips: 0 };
    mesh.scale.setScalar(p.base);
    if(ringP){
      Object.assign(p, mkPath(toPts(arcIds(p.arc))));
    }else{
      p.node = Math.floor(rand() * N);
      p.dest = near[p.node][Math.floor(rand() * 10)];
      Object.assign(p, mkPath(toPts(router.path(G.front[p.node], G.front[p.dest]))));
    }
    p.s = rand() * p.L;
    world.add(mesh);
    packets.push(p);
  }
  function nextRoute(p){
    if(p.ring){
      p.trips++;
      if(p.trips % 3 === 0){ p.arc = (p.arc + 1 + Math.floor(rand() * 4)) % G.spokeAt.length; Object.assign(p, mkPath(toPts(arcIds(p.arc)))); }
      else { const rev = p.pts.slice().reverse(); Object.assign(p, mkPath(rev)); }
    }else{
      p.node = p.dest;
      p.dest = near[p.node][Math.floor(rand() * 10)];
      Object.assign(p, mkPath(toPts(router.path(G.front[p.node], G.front[p.dest]))));
    }
    p.s = 0;
  }

  // ----- dust (desktop only) -----
  let dust = null, dpos = null, dvel = null;
  if(!PHONE){
    const dn = 320;
    dpos = new Float32Array(dn * 3); dvel = new Float32Array(dn * 3);
    for(let i = 0; i < dn; i++){
      const r = Math.sqrt(rand()) * 42, a = rand() * Math.PI * 2;
      dpos[i * 3] = Math.cos(a) * r; dpos[i * 3 + 1] = .3 + rand() * 8; dpos[i * 3 + 2] = Math.sin(a) * r;
      dvel[i * 3] = (rand() - .5) * .12; dvel[i * 3 + 1] = (rand() - .5) * .05; dvel[i * 3 + 2] = (rand() - .5) * .12;
    }
    const dg = new THREE.BufferGeometry(); dg.setAttribute('position', new THREE.BufferAttribute(dpos, 3));
    dust = new THREE.Points(dg, new THREE.PointsMaterial({ size: .07, map: dotTex, color: WARM, transparent: true, opacity: .42, depthWrite: false, blending: THREE.AdditiveBlending }));
    dust.frustumCulled = false;
    scene.add(dust);
  }

  // ----- camera -----
  const cam = { pos: new THREE.Vector3(), look: new THREE.Vector3() };
  const dPos = new THREE.Vector3(), dLook = new THREE.Vector3(), tmpD = new THREE.Vector3();
  const ptr = { x: 0, y: 0 }, ptrS = { x: 0, y: 0 };
  const state = { lit: new Map(), focusTool: null, pinned: null, time: 0, driftT: 0 };
  function driftTarget(t, outPos, outLook){
    const a = t * .032 + .4;
    outPos.set(Math.cos(a) * camR, eyeH + .1 * Math.sin(a * 2.7), Math.sin(a) * camR);
    const phi = a + Math.PI + .5 * Math.sin(a * 1.35 + .8);
    outLook.set(outPos.x + Math.cos(phi) * 30, 1.25, outPos.z + Math.sin(phi) * 30);
  }
  driftTarget(0, cam.pos, cam.look);
  const FOLLOW_BACK = PHONE ? 9 : 7.5, FOLLOW_H = eyeH + .35;
  let drop = null, bar = null;

  function pickView(){
    if(drop && drop.mesh && drop.state === 'travel'){
      const P = drop.mesh.position, D = drop.dir;
      dPos.set(P.x - D.x * FOLLOW_BACK, FOLLOW_H, P.z - D.z * FOLLOW_BACK);
      dLook.set(P.x + D.x * 4, .7, P.z + D.z * 4);
      return [1.0, 1.6];
    }
    const held = drop && drop.tool && (drop.state === 'dock' || drop.state === 'docked');
    const tool = held ? drop.tool : (state.focusTool || state.pinned);
    if(tool){
      const v = vantage[tool.i], s = stations[tool.i];
      dPos.set(v.x, eyeH + .15, v.z); dLook.set(s.x, .6, s.z);
      return [.85, 1.3];
    }
    driftTarget(state.driftT, dPos, dLook);
    return [1.2, 1.5];
  }

  // ----- dropped file: a physical card that rides the shortest rail path -----
  const dropSide = new THREE.MeshBasicMaterial({ color: WARM });
  const dropGeo = new THREE.BoxGeometry(.64, .88, .03);
  function syncBusy(){
    document.body.classList.toggle('busy', state.lit.size > 0 || !!state.pinned || !!drop);
  }
  function disposeDrop(d){
    d.dead = true;
    if(d.mesh){ world.remove(d.mesh); if(d.front) d.front.dispose(); if(d.tex) d.tex.dispose(); }
  }
  function clearDrop(){
    if(drop){ if(drop.tool) stations[drop.tool.i].dockT = 0; disposeDrop(drop); drop = null; }
    syncBusy();
  }
  function reset(){
    clearDrop();
    stations.forEach(s => { s.dockT = 0; });
    state.lit = new Map(); state.focusTool = null; state.pinned = null;
    syncBusy();
  }
  function buildDropMesh(d, cv){
    d.tex = tex(cv, aniso);
    d.front = new THREE.MeshBasicMaterial({ map: d.tex });
    d.mesh = new THREE.Mesh(dropGeo, [dropSide, dropSide, dropSide, dropSide, d.front, dropSide]);
    const g = new THREE.Sprite(glowMat); g.position.set(0, -.12, 0); g.scale.set(1.9, 1.9, 1);
    d.mesh.add(g);
    d.mesh.visible = false;
    world.add(d.mesh);
  }
  function startTravel(tool){
    const d = drop;
    if(!d || !d.mesh) return;
    if(d.tool) stations[d.tool.i].dockT = 0;
    d.tool = tool;
    camera.getWorldDirection(tmpD); tmpD.y = 0;
    if(tmpD.lengthSq() < 1e-6) tmpD.set(0, 0, -1);
    tmpD.normalize();
    const sx = camera.position.x + tmpD.x * 2.4, sz = camera.position.z + tmpD.z * 2.4;
    let best = -1, bd = 1e12;
    G.nodes.forEach((n, i) => {
      if(n.kind !== 's' && n.kind !== 'g') return;
      const dd = (n.x - sx) * (n.x - sx) + (n.z - sz) * (n.z - sz);
      if(dd < bd){ bd = dd; best = i; }
    });
    const ids = router.path(best, G.front[tool.i]);
    const pts = [{ x: sx, y: .62, z: sz }, ...ids.map(id => ({ x: G.nodes[id].x, y: .62, z: G.nodes[id].z }))];
    Object.assign(d, mkPath(pts));
    d.s = 0; d.age = 0; d.state = 'travel';
    d.dir = new THREE.Vector3().copy(tmpD);
    d.mesh.position.set(pts[0].x, pts[0].y, pts[0].z);
    d.mesh.scale.setScalar(.55);
    d.mesh.visible = true;
    if(bar) bar.setNote('');
    syncBusy();
  }
  async function onFile(file, kind){
    clearDrop();
    const m = bar && bar.matches && bar.matches[0];
    const dflt = (search('', kind)[0] || {}).tool;
    const tool = m ? m.tool : (dflt || TOOLS[0]);
    const mine = drop = { file, kind, tool, mesh: null, tex: null, front: null, state: 'paint', dead: false, dir: new THREE.Vector3(), age: 0, s: 0, L: 0, t: 0 };
    syncBusy();
    let cv = null;
    try{ const r = await paintFile(file, 'glass'); cv = r && r.canvas; }catch(err){ cv = null; }
    if(drop !== mine) return;
    buildDropMesh(mine, cv || paintCard(tool, 'glass'));
    startTravel(mine.tool);
  }
  function onPick(tool, file){
    if(file && drop){
      state.pinned = null;
      if(drop.mesh) startTravel(tool); else drop.tool = tool;
    }else{
      state.pinned = tool;
    }
    syncBusy();
  }
  function onChange(query, matches, hoverTool){
    if(bar && !bar.file && drop) clearDrop();
    const q = (query || '').trim();
    const lit = new Map();
    if(hoverTool) lit.set(hoverTool.i, 1);
    if(q && matches && matches.length){
      matches.slice(0, 3).forEach((m, k) => { if(!lit.has(m.tool.i)) lit.set(m.tool.i, [1, .65, .5][k]); });
      state.focusTool = hoverTool || matches[0].tool;
    }else{
      state.focusTool = hoverTool || null;
    }
    state.lit = lit;
    syncBusy();
  }
  bar = createBar({ mount: document.getElementById('bar'), placeholder: 'Drop a file, or type what you need', onChange, onFile, onPick });
  addEventListener('keydown', e => { if(e.key === 'Escape') reset(); });
  addEventListener('pointermove', e => { ptr.x = e.clientX / innerWidth * 2 - 1; ptr.y = e.clientY / innerHeight * 2 - 1; }, { passive: true });

  // ----- per frame -----
  const blueSoft = new THREE.Color(SOFTBLUE);
  function updateStations(dt){
    const pin = state.pinned ? state.pinned.i : -1;
    const dest = drop && drop.tool && drop.state === 'travel' ? drop.tool.i : -1;
    const hasLit = state.lit.size > 0 || pin >= 0;
    for(const s of stations){
      let tl = state.lit.get(s.i) || 0;
      if(s.i === pin) tl = 1;
      if(s.i === dest) tl = Math.max(tl, .8);
      const td = hasLit && tl === 0 && s.i !== dest ? 1 : 0;
      s.lit = damp(s.lit, tl, 3.2, dt); s.dim = damp(s.dim, td, 3.2, dt); s.dock = damp(s.dock, s.dockT, .7, dt);
      s.faceMat.color.setScalar(.78 + .5 * s.lit - .45 * s.dim + .18 * s.dock);
      const breathe = .04 * Math.sin(state.time * .9 + s.i * 1.7);
      s.ringMat.opacity = clamp(.5 + breathe + .5 * s.lit - .36 * s.dim + .5 * s.dock, 0, 1);
      s.ringMat.color.copy(warm).lerp(blueSoft, .55 * s.dock);
      const sc = 5.2 * (1 + .28 * s.lit + .2 * s.dock - .06 * s.dim);
      s.ring.scale.set(sc, 1, sc);
      s.haloMat.opacity = clamp(.14 + .22 * s.lit - .09 * s.dim + .14 * s.dock, 0, 1);
    }
  }
  function updatePackets(dt){
    for(const p of packets){
      if(p.pause > 0){
        p.pause -= dt;
        if(p.pause <= 0) nextRoute(p);
      }else{
        const e = Math.min(p.s, p.L - p.s);
        p.s = Math.min(p.L, p.s + p.v * (.16 + .84 * sstep(0, 2.4, e)) * dt);
        if(p.s >= p.L - 1e-3) p.pause = 1.1 + rand() * 2.6;
      }
      const m = p.mesh;
      sample(p.pts, p.cum, p.s, m.position, null);
      m.position.y = .42 * p.base + .04 * Math.sin(state.time * 1.7 + p.phase);
      m.rotation.y = Math.atan2(camera.position.x - m.position.x, camera.position.z - m.position.z);
      const gs = (p.pause > 0 ? .7 : 1) * (p.ring ? 1.9 : 1.5);
      p.glow.scale.set(gs, gs, 1);
    }
  }
  function updateDrop(dt){
    const d = drop;
    if(!d || !d.mesh || !d.mesh.visible) return;
    d.age += dt;
    const m = d.mesh;
    const yawCam = Math.atan2(camera.position.x - m.position.x, camera.position.z - m.position.z);
    if(d.state === 'travel'){
      const e = Math.min(d.s, d.L - d.s);
      d.s = Math.min(d.L, d.s + 5.4 * (.2 + .8 * sstep(0, 2.6, e)) * dt);
      sample(d.pts, d.cum, d.s, m.position, tmpD);
      if(tmpD.lengthSq() > 0) d.dir.lerp(tmpD, 1 - Math.exp(-3 * dt));
      if(d.dir.lengthSq() < 1e-6) d.dir.set(0, 0, -1);
      d.dir.normalize();
      m.position.y = .62 + .05 * Math.sin(d.age * 2.2);
      m.rotation.y = yawCam;
      m.scale.setScalar(.55 + .45 * sstep(0, .8, d.age));
      if(d.s >= d.L - 1e-3){
        d.state = 'dock'; d.t = 0;
        stations[d.tool.i].dockT = 1;
        if(bar) bar.setNote(MESSAGE);
      }
    }else if(d.state === 'dock'){
      const st = stations[d.tool.i];
      d.t = Math.min(1, d.t + dt / 2.4);
      const zl = lerp(1.3, -.5, sstep(.25, 1, d.t));
      const yl = lerp(.62, 1.17, sstep(0, .55, d.t));
      m.position.set(st.x + st.fx * zl, yl, st.z + st.fz * zl);
      m.rotation.y = lerpAngle(yawCam, st.yaw, sstep(0, .45, d.t));
      if(d.t >= 1) d.state = 'docked';
    }
  }
  function updateDust(dt){
    if(!dust || REDUCED) return;
    for(let i = 0; i < dpos.length; i += 3){
      dpos[i] += dvel[i] * dt; dpos[i + 1] += dvel[i + 1] * dt; dpos[i + 2] += dvel[i + 2] * dt;
      if(dpos[i + 1] < .2 || dpos[i + 1] > 9) dvel[i + 1] *= -1;
      if(dpos[i] * dpos[i] + dpos[i + 2] * dpos[i + 2] > 44 * 44){ dvel[i] *= -1; dvel[i + 2] *= -1; }
    }
    dust.geometry.attributes.position.needsUpdate = true;
  }
  function updateCamera(dt){
    if(!REDUCED) state.driftT += dt;
    const k = pickView();
    const fp = 1 - Math.exp(-k[0] * dt), fl = 1 - Math.exp(-k[1] * dt);
    cam.pos.lerp(dPos, fp); cam.look.lerp(dLook, fl);
    // keep clear of tablets and inside the world
    for(const s of stations){
      const dx = cam.pos.x - s.x, dz = cam.pos.z - s.z, dd = Math.hypot(dx, dz);
      if(dd < 1.9 && dd > 1e-4){ cam.pos.x = s.x + dx / dd * 1.9; cam.pos.z = s.z + dz / dd * 1.9; }
    }
    const rr = Math.hypot(cam.pos.x, cam.pos.z);
    if(rr > 58){ cam.pos.x *= 58 / rr; cam.pos.z *= 58 / rr; }
    ptrS.x = damp(ptrS.x, ptr.x, 2.5, dt); ptrS.y = damp(ptrS.y, ptr.y, 2.5, dt);
    camera.position.copy(cam.pos);
    camera.lookAt(cam.look);
    if(!REDUCED){ camera.rotateY(-ptrS.x * .03); camera.rotateX(-ptrS.y * .018); }
  }

  function renderFrame(){
    renderer.clear();
    if(MIRROR){
      floor.visible = false; wall.visible = true; if(dust) dust.visible = false;
      world.scale.y = -1;
      renderer.render(scene, camera);
      renderer.clearDepth();
      world.scale.y = 1; floor.visible = true; wall.visible = false; if(dust) dust.visible = true;
      renderer.render(scene, camera);
      wall.visible = true;
    }else{
      renderer.render(scene, camera);
    }
  }

  function onResize(){
    const w = Math.max(1, innerWidth), h = Math.max(1, innerHeight);
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.fov = w / h < .9 ? 60 : 52;
    camera.updateProjectionMatrix();
  }
  addEventListener('resize', onResize);
  onResize();

  window.__nl = { scene, camera, state, stations, packets, G, getDrop: () => drop, renderer };

  let last = performance.now(), first = true;
  function frame(now){
    requestAnimationFrame(frame);
    const dt = clamp((now - last) / 1000, 0, .05);
    last = now;
    state.time += dt;
    wallMat.uniforms.uTime.value = state.time;
    railMat.opacity = .38 + .04 * Math.sin(state.time * .5);
    updateStations(dt);
    updateDrop(dt);
    updateCamera(dt);
    updatePackets(dt);
    updateDust(dt);
    renderFrame();
    if(first){ first = false; document.documentElement.dataset.ready = '1'; }
  }
  requestAnimationFrame(frame);
}

main().catch(err => {
  console.error(err);
  document.documentElement.dataset.error = '1';
  document.documentElement.dataset.ready = '1';
});
