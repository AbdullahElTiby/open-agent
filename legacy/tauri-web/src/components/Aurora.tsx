/** The living backdrop: drifting aurora ribbons, a masked grid and film grain. */
export function Aurora() {
  return (
    <div className="aurora" aria-hidden>
      <div className="aurora-blob b1" />
      <div className="aurora-blob b2" />
      <div className="aurora-blob b3" />
      <div className="aurora-blob b4" />
      <div className="aurora-ribbon" />
      <div className="aurora-grid" />
      <div className="aurora-grain" />
      <div className="aurora-vignette" />
    </div>
  );
}
