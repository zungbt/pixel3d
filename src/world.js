import * as THREE from 'three';

export const SIZE = 48; // large enough to fill a 21:9 screen at VIEW_HEIGHT 13

// Seeded PRNG so the layout is identical on every load.
export function mulberry32(seed) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function baseHeight(x, z) {
  return 0.35 * Math.sin(x * 0.6) * Math.cos(z * 0.5) + 0.15 * Math.sin((x + z) * 0.9);
}

// Pond: an irregular water polygon inside a wide sand beach, both lying on terrain
// flattened around them, so neither ever intersects sloped triangles (nothing is carved).
const POND_X = 6.2;
const POND_Z = -2.3;
const POND_R_MAX = 3.7 * 1.22;
const SAND_R_MAX = POND_R_MAX * 1.1;
const FLAT_R = SAND_R_MAX + 1.5; // + one grid cell diagonal: every triangle under the sand is flat
const FLAT_BLEND = 3;
const SHORE_Y = baseHeight(POND_X, POND_Z);

const WATER_DEEP = new THREE.Color(0x3a8fc9);
const WATER_MID = new THREE.Color(0x4fb0dc);
const WATER_SHALLOW = new THREE.Color(0x6fcbe6);
const WATER_BANK = new THREE.Color(0x2c5a7a);
const WATER_FOAM = new THREE.Color(0xeef6ff);
const waterUniforms = { uWaterTime: { value: 0 } };

function pondRadius(theta) {
  return 3.7 * (1 + 0.15 * Math.sin(3 * theta + 1) + 0.07 * Math.sin(5 * theta + 2));
}

// Beach width varies around the pond: wide on some sides, narrow on others.
function sandRadius(theta) {
  return pondRadius(theta) * (1.07 + 0.02 * Math.sin(2 * theta + 0.5) + 0.01 * Math.sin(3 * theta + 2));
}

function pondPolar(x, z) {
  const dx = x - POND_X;
  const dz = z - POND_Z;
  return [Math.hypot(dx, dz), Math.atan2(dz, dx)];
}

// Distance past the shoreline in world units: < 0 is water.
export function pondEdge(x, z) {
  const [d, theta] = pondPolar(x, z);
  return d - pondRadius(theta);
}

// The point `offset` units out from the shoreline in direction theta (< 0 is in the water).
export function pondPoint(theta, offset) {
  const r = pondRadius(theta) + offset;
  return [POND_X + Math.cos(theta) * r, POND_Z + Math.sin(theta) * r];
}

// Same, relative to the edge of the beach: < 1 is sand (or water).
export function sandQ(x, z) {
  const [d, theta] = pondPolar(x, z);
  return d / sandRadius(theta);
}

// Shared by the terrain and by prop placement so nothing floats or sinks.
export function heightAt(x, z) {
  const d = Math.hypot(x - POND_X, z - POND_Z);
  const flat = 1 - THREE.MathUtils.smoothstep(d, FLAT_R, FLAT_R + FLAT_BLEND);
  return THREE.MathUtils.lerp(baseHeight(x, z), SHORE_Y, flat);
}

// Dirt path: a meandering centreline x = pathX(z), which runs bottom-left to top-right
// on screen and passes left of the pond. The same constants feed the JS and GLSL versions.
const PATH = { x0: -2.8, amp: 2, freq: 0.22, phase: 1 };
const PATH_W = { base: 0.55, a1: 0.1, f1: 2.3, a2: 0.06, f2: 5.1 }; // ragged half-width
const PATH_COLOR = new THREE.Color(0xc3a284);

function pathHalfW(x, z) {
  return PATH_W.base + PATH_W.a1 * Math.sin(z * PATH_W.f1) + PATH_W.a2 * Math.sin(z * PATH_W.f2 + x);
}

// Approximate distance to the centreline (horizontal offset scaled by the curve's slope).
function pathDist(x, z) {
  const a = z * PATH.freq + PATH.phase;
  const slope = PATH.amp * PATH.freq * Math.cos(a);
  return Math.abs(x - (PATH.x0 + PATH.amp * Math.sin(a))) / Math.sqrt(1 + slope * slope);
}

function mainPathEdge(x, z) {
  return pathDist(x, z) - pathHalfW(x, z);
}

// Branch path: leaves the main path left of the pond, runs between the pond and the rocks
// along z = z0 + a * (1 - exp(-(x - x0) / a)), and narrows into the grass at x1. It was added
// after the layout was fixed, so trees and rocks are placed against the main path only (none
// stands on the branch) and keep their places. Starts at the main path's centreline, so its
// cut-off end is hidden inside the main path.
const BRANCH = { z0: 1.4, a: 5.5, x1: 15, taper: 3, width: 0.82 };
const BRANCH_X0 = PATH.x0 + PATH.amp * Math.sin(BRANCH.z0 * PATH.freq + PATH.phase);

function branchEdge(x, z) {
  if (x < BRANCH_X0) return Infinity;
  const k = Math.exp(-(x - BRANCH_X0) / BRANCH.a); // also the curve's slope
  const dist = Math.abs(z - (BRANCH.z0 + BRANCH.a * (1 - k))) / Math.sqrt(1 + k * k);
  // Same ragged edge as the main path, varying along x since the branch runs along x.
  const halfW = pathHalfW(z, x) * BRANCH.width * (1 - THREE.MathUtils.smoothstep(x, BRANCH.x1 - BRANCH.taper, BRANCH.x1));
  return dist - halfW;
}

// Distance from the edge of either path: < 0 is on a path.
export function pathEdge(x, z) {
  return Math.min(mainPathEdge(x, z), branchEdge(x, z));
}

// Smooth 2D value noise in [0, 1], used as density maps for placement.
function hash2(x, z) {
  const s = Math.sin(x * 127.1 + z * 311.7) * 43758.5453;
  return s - Math.floor(s);
}

function valueNoise(x, z) {
  const ix = Math.floor(x);
  const iz = Math.floor(z);
  const fx = x - ix;
  const fz = z - iz;
  const ux = fx * fx * (3 - 2 * fx);
  const uz = fz * fz * (3 - 2 * fz);
  const a = THREE.MathUtils.lerp(hash2(ix, iz), hash2(ix + 1, iz), ux);
  const b = THREE.MathUtils.lerp(hash2(ix, iz + 1), hash2(ix + 1, iz + 1), ux);
  return THREE.MathUtils.lerp(a, b, uz);
}

export function fbm(x, z) {
  return valueNoise(x, z) * 0.65 + valueNoise(x * 2.1 + 17, z * 2.1 + 31) * 0.35;
}

// GLSL value noise shared by the water and cloud patches. The guard matters: the water
// shader receives this chunk from both patches.
export const NOISE_GLSL = /* glsl */ `
  #ifndef NOISE_GLSL
  #define NOISE_GLSL
  float noiseHash( vec2 p ) {
    return fract( sin( dot( p, vec2( 127.1, 311.7 ) ) ) * 43758.5453 );
  }

  float valueNoise( vec2 p ) {
    vec2 i = floor( p );
    vec2 f = fract( p );
    vec2 u = f * f * ( 3.0 - 2.0 * f );
    return mix(
      mix( noiseHash( i ), noiseHash( i + vec2( 1.0, 0.0 ) ), u.x ),
      mix( noiseHash( i + vec2( 0.0, 1.0 ) ), noiseHash( i + vec2( 1.0, 1.0 ) ), u.x ),
      u.y
    );
  }
  #endif
`;

function slopeAt(x, z) {
  const e = 0.1;
  return Math.hypot(heightAt(x + e, z) - heightAt(x - e, z), heightAt(x, z + e) - heightAt(x, z - e)) / (2 * e);
}

function toonGradient() {
  const tex = new THREE.DataTexture(new Uint8Array([60, 125, 195, 255]), 4, 1, THREE.RedFormat);
  tex.minFilter = THREE.NearestFilter;
  tex.magFilter = THREE.NearestFilter;
  tex.generateMipmaps = false;
  tex.needsUpdate = true;
  return tex;
}

function buildGround(gradientMap) {
  let geo = new THREE.PlaneGeometry(SIZE, SIZE, SIZE, SIZE);
  geo.rotateX(-Math.PI / 2);
  const pos = geo.attributes.position;
  for (let i = 0; i < pos.count; i++) {
    pos.setY(i, heightAt(pos.getX(i), pos.getZ(i)));
  }
  // Soft light/dark patches so bare ground isn't one flat tone.
  const tint = [];
  for (let i = 0; i < pos.count; i++) {
    const k = 0.9 + 0.18 * fbm(pos.getX(i) * 0.2 + 80, pos.getZ(i) * 0.2);
    tint.push(k, k, k);
  }
  geo.setAttribute('color', new THREE.Float32BufferAttribute(tint, 3));
  // Non-indexed so each face gets its own flat normal (the pixel pass reads geometry normals).
  geo = geo.toNonIndexed();
  geo.computeVertexNormals();

  const material = new THREE.MeshToonMaterial({ color: 0xa4cc86, gradientMap, vertexColors: true });
  const f = (n) => n.toFixed(4);
  material.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec2 vGroundXZ;')
      .replace('#include <begin_vertex>', '#include <begin_vertex>\nvGroundXZ = ( modelMatrix * vec4( transformed, 1.0 ) ).xz;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying vec2 vGroundXZ;')
      .replace('#include <color_fragment>', /* glsl */ `
        #include <color_fragment>
        {
          float x = vGroundXZ.x;
          float z = vGroundXZ.y;
          float a = z * ${f(PATH.freq)} + ${f(PATH.phase)};
          float slope = ${f(PATH.amp * PATH.freq)} * cos( a );
          float dist = abs( x - ( ${f(PATH.x0)} + ${f(PATH.amp)} * sin( a ) ) ) / sqrt( 1.0 + slope * slope );
          float halfW = ${f(PATH_W.base)} + ${f(PATH_W.a1)} * sin( z * ${f(PATH_W.f1)} ) + ${f(PATH_W.a2)} * sin( z * ${f(PATH_W.f2)} + x );
          bool onPath = dist < halfW;
          float bx = x - ${f(BRANCH_X0)};
          if ( bx > 0.0 ) {
            float k = exp( -bx / ${f(BRANCH.a)} );
            float bDist = abs( z - ( ${f(BRANCH.z0)} + ${f(BRANCH.a)} * ( 1.0 - k ) ) ) / sqrt( 1.0 + k * k );
            float bHalfW = ( ${f(PATH_W.base)} + ${f(PATH_W.a1)} * sin( x * ${f(PATH_W.f1)} ) + ${f(PATH_W.a2)} * sin( x * ${f(PATH_W.f2)} + z ) )
              * ${f(BRANCH.width)} * ( 1.0 - smoothstep( ${f(BRANCH.x1 - BRANCH.taper)}, ${f(BRANCH.x1)}, x ) );
            onPath = onPath || bDist < bHalfW;
          }
          if ( onPath ) diffuseColor.rgb = ${glslColor(PATH_COLOR)} * vColor.rgb;
        }
      `);
  };
  const mesh = new THREE.Mesh(geo, material);
  mesh.receiveShadow = true;
  return mesh;
}

// Unit-sized geometry shared by every rock and tree; each mesh is sized through its scale.
const ROCK_GEO = new THREE.DodecahedronGeometry(1, 0);
const TRUNK_GEO = new THREE.CylinderGeometry(0.12, 0.16, 1, 6);
const LEAVES_GEO = new THREE.IcosahedronGeometry(1, 0);

function buildRock(rand, r, material) {
  const mesh = new THREE.Mesh(ROCK_GEO, material);
  mesh.scale.set(r, r * (0.6 + rand() * 0.4), r);
  mesh.rotation.y = rand() * Math.PI;
  mesh.userData.radius = r;
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

function buildTree(rand, materials) {
  const tree = new THREE.Group();
  const height = 0.8 + rand() * 0.6;

  const trunk = new THREE.Mesh(TRUNK_GEO, materials.trunk);
  trunk.scale.y = height;
  trunk.position.y = height / 2;

  const canopy = 0.6 + rand() * 0.3;
  tree.userData.canopy = canopy;
  tree.userData.crown = height + 0.3; // crown centre above the base
  const leaves = new THREE.Mesh(LEAVES_GEO, materials.leaves);
  leaves.scale.setScalar(canopy);
  leaves.position.y = height + 0.3;

  for (const part of [trunk, leaves]) {
    part.castShadow = true;
    part.receiveShadow = true;
    tree.add(part);
  }
  return tree;
}

// Triangle fan around the pond centre; aEdge is 0 at the centre and 1 on the rim, so
// once interpolated it is the fragment's distance relative to the outline.
function buildPondShape(radiusAt, y, material) {
  const geo = new THREE.CircleGeometry(1, 72);
  geo.rotateX(-Math.PI / 2);
  const pos = geo.attributes.position;
  const edge = new Float32Array(pos.count);
  for (let i = 0; i < pos.count; i++) {
    edge[i] = Math.hypot(pos.getX(i), pos.getZ(i));
    const r = radiusAt(Math.atan2(pos.getZ(i), pos.getX(i)));
    pos.setXYZ(i, pos.getX(i) * r, 0, pos.getZ(i) * r);
  }
  geo.setAttribute('aEdge', new THREE.BufferAttribute(edge, 1));
  const mesh = new THREE.Mesh(geo, material);
  mesh.position.set(POND_X, y, POND_Z);
  mesh.receiveShadow = true;
  return mesh;
}

const glslColor = (c) => `vec3(${c.r.toFixed(4)}, ${c.g.toFixed(4)}, ${c.b.toFixed(4)})`;

// Stepped contour bands, a dark far bank (fakes water sitting below the ground),
// broken foam along the rim and twinkling glints. Colours still go through toon lighting.
function waterMaterial(gradientMap) {
  const material = new THREE.MeshToonMaterial({ gradientMap });
  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, waterUniforms);
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float aEdge;\nvarying float vEdge;\nvarying vec2 vPondXZ;')
      .replace('#include <begin_vertex>', '#include <begin_vertex>\nvEdge = aEdge;\nvPondXZ = position.xz;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', /* glsl */ `#include <common>
        uniform float uWaterTime;
        varying float vEdge;
        varying vec2 vPondXZ;
        ${NOISE_GLSL}`)
      .replace('#include <color_fragment>', /* glsl */ `#include <color_fragment>
        {
        float t = uWaterTime;
        float e = vEdge + ( valueNoise( vPondXZ * 1.3 ) - 0.5 ) * 0.12; // wobbly, not concentric
        vec3 water = e < 0.45 ? ${glslColor(WATER_DEEP)} : e < 0.75 ? ${glslColor(WATER_MID)} : ${glslColor(WATER_SHALLOW)};
        // Far side from the camera (world -x,-z): the inner bank wall shows as a dark band.
        float far = dot( vPondXZ, vec2( -0.7071 ) ) / max( length( vPondXZ ), 1e-4 );
        float bank = smoothstep( 0.2, 1.0, far ) * 0.16;
        bool isBank = vEdge > 1.0 - bank;
        float foamEdge = 0.95 - valueNoise( vPondXZ * 4.0 + vec2( t * 0.3, 0.0 ) ) * 0.1;
        bool isFoam = !isBank && ( vEdge > foamEdge || ( vEdge > 0.8 && noiseHash( floor( vPondXZ * 12.0 ) ) > 0.97 ) );
        // Glints: short dashes along the screen horizontal (world x - z), drifting slowly.
        vec2 sp = vec2( ( vPondXZ.x - vPondXZ.y ) * 0.7071 + t * 0.08, ( vPondXZ.x + vPondXZ.y ) * 0.7071 );
        float h = noiseHash( floor( sp * vec2( 4.0, 12.0 ) ) );
        bool isGlint = vEdge < 0.85 && h > 0.97 && sin( t * 2.0 + h * 60.0 ) > 0.5;
        if ( isBank ) water = ${glslColor(WATER_BANK)};
        if ( isFoam || isGlint ) water = ${glslColor(WATER_FOAM)};
        diffuseColor.rgb *= water;
        }`);
  };
  return material;
}

function placeOnGround(obj, x, z) {
  obj.position.set(x, heightAt(x, z), z);
}

export function buildWorld(scene) {
  const rand = mulberry32(42);
  const gradientMap = toonGradient();
  const materials = {
    trunk: new THREE.MeshToonMaterial({ color: 0x8d6d52, gradientMap }),
    leaves: new THREE.MeshToonMaterial({ color: 0x589e63, gradientMap }),
    rock: new THREE.MeshToonMaterial({ color: 0xaaafc2, gradientMap }),
  };

  scene.add(buildGround(gradientMap));
  scene.add(buildPondShape(sandRadius, SHORE_Y + 0.01, new THREE.MeshToonMaterial({ color: 0xecd8b8, gradientMap })));
  const water = buildPondShape(pondRadius, SHORE_Y + 0.02, waterMaterial(gradientMap));
  scene.add(water);

  const spread = () => (rand() - 0.5) * (SIZE - 3);
  const tooClose = (x, z, minDist) =>
    trees.some((t) => Math.hypot(t.position.x - x, t.position.z - z) < minDist);

  // Trees: groves where the forest mask is high, rare loners in clearings,
  // and a minimum spacing between trunks (dart throwing ≈ Poisson-disk).
  const trees = [];
  for (let attempt = 0; attempt < 4000 && trees.length < 120; attempt++) {
    const x = spread();
    const z = spread();
    const forest = THREE.MathUtils.smoothstep(fbm(x * 0.09, z * 0.09), 0.45, 0.65);
    if (rand() > Math.max(forest, 0.01) || tooClose(x, z, 1.6) || sandQ(x, z) < 1.15 || mainPathEdge(x, z) < 1) continue;
    const tree = buildTree(rand, materials);
    placeOnGround(tree, x, z);
    tree.userData.phase = trees.length * 2.4;
    trees.push(tree);
  }

  // Rocks: clusters of one boulder plus a few small stones, favouring slopes, never inside a tree.
  // The centre check uses the largest boulder radius, so every counted cluster keeps its boulder.
  const rocks = [];
  for (let clusters = 0, attempt = 0; clusters < 12 && attempt < 500; attempt++) {
    const cx = spread();
    const cz = spread();
    if (rand() > 0.3 + slopeAt(cx, cz) * 1.5 || tooClose(cx, cz, 1.5) || mainPathEdge(cx, cz) < 0.9 + 0.15 || pondEdge(cx, cz) < 0.9) continue;
    clusters++;
    const stones = [[cx, cz, 0.6 + rand() * 0.3]];
    const small = 2 + Math.floor(rand() * 4);
    for (let i = 0; i < small; i++) {
      const a = rand() * Math.PI * 2;
      const d = 0.6 + rand() * 1.0;
      stones.push([cx + Math.cos(a) * d, cz + Math.sin(a) * d, 0.2 + rand() * 0.15]);
    }
    for (const [x, z, r] of stones) {
      if (tooClose(x, z, 0.7) || pondEdge(x, z) < r || mainPathEdge(x, z) < r + 0.15) continue;
      const rock = buildRock(rand, r, materials.rock);
      placeOnGround(rock, x, z);
      rocks.push(rock);
    }
  }

  // Trees and rocks stay plain objects (insects, flowers and grass read their positions) but are
  // drawn as three instanced meshes: one draw call per pass each instead of ~300 in all.
  const instanced = (geo, material, parts) => {
    const mesh = new THREE.InstancedMesh(geo, material, parts.length);
    parts.forEach((part, i) => {
      part.updateWorldMatrix(true, false);
      mesh.setMatrixAt(i, part.matrixWorld);
    });
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    scene.add(mesh);
    return mesh;
  };
  const trunks = instanced(TRUNK_GEO, materials.trunk, trees.map((t) => t.children[0]));
  const crowns = instanced(LEAVES_GEO, materials.leaves, trees.map((t) => t.children[1]));
  instanced(ROCK_GEO, materials.rock, rocks);

  return {
    trees,
    rocks,
    gradientMap,
    update(t) {
      // Water picks up a hint of the sky colour (sky.update runs after this, so it lags one frame).
      water.material.color.set(0xffffff).lerp(scene.background, 0.1);
      waterUniforms.uWaterTime.value = t;
      // Group origin sits on the ground, so the tree pivots at its base.
      trees.forEach((tree, i) => {
        const p = tree.userData.phase;
        tree.rotation.z = 0.035 * Math.sin(t * 1.3 + p);
        tree.rotation.x = 0.02 * Math.sin(t * 0.9 + p * 1.7);
        tree.updateMatrixWorld();
        trunks.setMatrixAt(i, tree.children[0].matrixWorld);
        crowns.setMatrixAt(i, tree.children[1].matrixWorld);
      });
      trunks.instanceMatrix.needsUpdate = true;
      crowns.instanceMatrix.needsUpdate = true;
    },
  };
}
