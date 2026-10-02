import * as THREE from 'three';

const cloudUniforms = { uCloudTime: { value: 0 } };

const CLOUD_VERTEX = /* glsl */ `
  #include <worldpos_vertex>
  vec4 cloudPos = vec4( transformed, 1.0 );
  #ifdef USE_INSTANCING
    cloudPos = instanceMatrix * cloudPos;
  #endif
  vCloudPos = ( modelMatrix * cloudPos ).xyz;
`;

// Hard-edged value-noise blobs drifting with the wind; they only dim direct sunlight.
const CLOUD_PARS = /* glsl */ `
  uniform float uCloudTime;
  varying vec3 vCloudPos;

  float cloudHash( vec2 p ) {
    return fract( sin( dot( p, vec2( 127.1, 311.7 ) ) ) * 43758.5453 );
  }

  float cloudNoise( vec2 p ) {
    vec2 i = floor( p );
    vec2 f = fract( p );
    vec2 u = f * f * ( 3.0 - 2.0 * f );
    return mix(
      mix( cloudHash( i ), cloudHash( i + vec2( 1.0, 0.0 ) ), u.x ),
      mix( cloudHash( i + vec2( 0.0, 1.0 ) ), cloudHash( i + vec2( 1.0, 1.0 ) ), u.x ),
      u.y
    );
  }

  float cloudShade( vec3 worldPos ) {
    vec2 p = worldPos.xz * 0.15 + vec2( uCloudTime * 0.05, uCloudTime * 0.02 );
    float n = cloudNoise( p ) * 0.65 + cloudNoise( p * 2.3 ) * 0.35;
    return mix( 1.0, 0.35, smoothstep( 0.58, 0.6, n ) );
  }
`;

function addCloudShadow(material) {
  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, cloudUniforms);
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vCloudPos;')
      .replace('#include <worldpos_vertex>', CLOUD_VERTEX);
    shader.fragmentShader = shader.fragmentShader.replace(
      '#include <lights_toon_pars_fragment>',
      CLOUD_PARS +
        THREE.ShaderChunk.lights_toon_pars_fragment.replace(
          '* directLight.color;',
          '* directLight.color * cloudShade( vCloudPos );',
        ),
    );
  };
}

// Call after everything that should receive cloud shadows is in the scene.
export function buildSky(scene) {
  scene.traverse((obj) => {
    if (obj.material?.isMeshToonMaterial) addCloudShadow(obj.material);
  });

  scene.add(new THREE.HemisphereLight(0xbfd8ff, 0x3a3326, 1.2));

  const sun = new THREE.DirectionalLight(0xfff1d6, 2.5);
  sun.position.set(8, 12, 4);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  sun.shadow.camera.left = -10;
  sun.shadow.camera.right = 10;
  sun.shadow.camera.top = 10;
  sun.shadow.camera.bottom = -10;
  sun.shadow.camera.near = 1;
  sun.shadow.camera.far = 40;
  sun.shadow.bias = -0.0005;
  sun.shadow.normalBias = 0.02;
  scene.add(sun);

  return {
    update(t) {
      cloudUniforms.uCloudTime.value = t;
    },
  };
}
