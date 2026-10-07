# pixel3d

A pixel-art 3D scene built with [three.js](https://threejs.org/) + [Vite]: windswept grass, wildflowers, butterflies, dragonflies, fireflies and gnats, a pond, a dirt path, trees, rocks, clouds and a day–night cycle, rendered through `RenderPixelatedPass`.

## Running

```bash
npm install
npm run dev        # http://127.0.0.1:5173/
npm run build      # build into dist/
npm test           # headless behaviour tests (node --test)
npm run preview    # serve the build
```

## Layout

| File | Contents |
|---|---|
| `src/main.js` | Renderer, orthographic camera (`VIEW_HEIGHT`, `PIXEL_SIZE`), post-processing, render loop |
| `src/world.js` | Ground, pond, dirt path, trees, rocks; map `SIZE`; shared noise/PRNG |
| `src/grass.js` | Instanced grass + wind shader |
| `src/flowers.js` | Instanced wildflower patches, swaying with the grass |
| `src/insects.js` | Builds every insect species; shared flight helpers (steering, obstacle avoidance) |
| `src/butterflies.js` | Butterflies: flower to flower, feeding, basking on rocks, roosting at night |
| `src/dragonflies.js` | Dragonflies: pond-rim territories, darts and hovers, perching, settling at night |
| `src/fireflies.js` | Fireflies: dusk-to-midnight swarms, fading J flashes, females answering males |
| `src/gnats.js` | Gnats: dusk and dawn swarms over the pond rim, scattered by hunting dragonflies |
| `src/motion.js` | Shared motion helpers: per-insect random streams, easing, turning spring, weighted choice |
| `src/frame-limiter.js` | Frame-rate cap behind the fps selector |
| `src/sky.js` | Sun, day–night cycle, clouds and cloud shadows |

## Controls

Bottom-left: a time-of-day slider (jumps the sky clock) and a frame-rate cap (Native / 60 / 30 / 20, remembered).

## Conventions

Branches, commits, versioning and releases: see [CONTRIBUTING.md](CONTRIBUTING.md).

[Vite]: https://vite.dev/
