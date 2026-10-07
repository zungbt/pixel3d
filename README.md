# pixel3d

A pixel-art 3D scene built with [three.js](https://threejs.org/) + [Vite]: windswept grass, a pond, a dirt path, trees, rocks, clouds and a day–night cycle, rendered through `RenderPixelatedPass`.

## Running

```bash
npm install
npm run dev        # http://127.0.0.1:5173/
npm run build      # build into dist/
npm run preview    # serve the build
```

## Layout

| File | Contents |
|---|---|
| `src/main.js` | Renderer, orthographic camera (`VIEW_HEIGHT`, `PIXEL_SIZE`), post-processing, render loop |
| `src/world.js` | Ground, pond, dirt path, trees, rocks; map `SIZE`; shared noise/PRNG |
| `src/grass.js` | Instanced grass + wind shader |
| `src/sky.js` | Sun, day–night cycle, clouds and cloud shadows |

## Conventions

Branches, commits, versioning and releases: see [CONTRIBUTING.md](CONTRIBUTING.md).

[Vite]: https://vite.dev/
