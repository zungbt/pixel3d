# pixel3d

Cảnh 3D pixel-art viết bằng [three.js](https://threejs.org/) + [Vite]: đồng cỏ có gió, ao, đường đất, cây, đá, mây và chu kỳ ngày–đêm, render qua `RenderPixelatedPass`.

## Chạy

```bash
npm install
npm run dev        # http://127.0.0.1:5173/
npm run build      # build ra dist/
npm run preview    # xem bản build
```

## Cấu trúc

| File | Nội dung |
|---|---|
| `src/main.js` | Renderer, camera orthographic (`VIEW_HEIGHT`, `PIXEL_SIZE`), post-processing, vòng lặp render |
| `src/world.js` | Mặt đất, ao, đường đất, cây, đá; `SIZE` của map; noise/PRNG dùng chung |
| `src/grass.js` | Cỏ instanced + shader gió |
| `src/sky.js` | Mặt trời, chu kỳ ngày–đêm, mây và bóng mây |

## Quy ước làm việc

Branch, commit, version và release: xem [CONTRIBUTING.md](CONTRIBUTING.md).

[Vite]: https://vite.dev/
