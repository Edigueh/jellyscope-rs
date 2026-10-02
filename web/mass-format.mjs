export function formatLogMass(value) {
  const mass = Number(value);
  return Number.isFinite(mass) && mass > 0 ? Math.log10(mass).toFixed(3) : "—";
}
