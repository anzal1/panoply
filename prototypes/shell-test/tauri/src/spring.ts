/**
 * Critically damped spring (damping ratio exactly 1, so no overshoot ever).
 * Closed form per step, so it is frame-rate independent and keeps velocity when
 * the target changes mid-flight: the pill never snaps, it only re-aims.
 */
export class Spring {
  value: number;
  velocity = 0;
  target: number;
  /** angular frequency; 26.5 rad/s settles to 2% in about 220 ms */
  omega: number;

  constructor(value: number, omega = 26.5) {
    this.value = value;
    this.target = value;
    this.omega = omega;
  }

  get active(): boolean {
    return Math.abs(this.value - this.target) > 0.05 || Math.abs(this.velocity) > 1;
  }

  jump(v: number): void {
    this.value = v;
    this.target = v;
    this.velocity = 0;
  }

  step(dt: number): void {
    const w = this.omega;
    const a = this.value - this.target;
    const b = this.velocity + w * a;
    const e = Math.exp(-w * dt);
    this.value = this.target + (a + b * dt) * e;
    this.velocity = (b - w * (a + b * dt)) * e;
    if (!this.active) {
      this.value = this.target;
      this.velocity = 0;
    }
  }
}
