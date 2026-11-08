/** The agent's "core": a glass sphere with orbiting light, breathing faster while it works. */
export function Orb({ size = 168, small = false }: { size?: number; small?: boolean }) {
  return (
    <div className={`orb ${small ? "orb-small" : ""}`} style={{ width: size, height: size }} aria-hidden>
      <div className="orb-halo" />
      <div className="orb-ring r1" />
      <div className="orb-ring r2" />
      <div className="orb-core">
        <div className="orb-swirl" />
        <div className="orb-shine" />
      </div>
      {!small && (
        <>
          <div className="orb-ripple" />
          <div className="orb-ripple d2" />
        </>
      )}
    </div>
  );
}
