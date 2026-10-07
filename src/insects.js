// Every insect species; built before the sky so their toon materials get cloud shadows.
export function buildInsects(scene, world) {
  const species = [];
  return {
    update(t, dt, sky) {
      for (const s of species) s.update(t, dt, sky);
    },
  };
}
