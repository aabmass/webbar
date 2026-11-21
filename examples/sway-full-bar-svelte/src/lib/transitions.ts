import { cubicOut } from "svelte/easing";
import type { TransitionConfig } from "svelte/transition";

export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

export function motionDuration(ms: number): number {
  return prefersReducedMotion() ? 0 : ms;
}

/**
 * Custom Svelte transition for top-layer popovers: animates both enter and
 * exit with opacity, vertical lift, and subtle scale while preserving any
 * inline `transform` (such as `translateX(-50%)` for center-aligned panels).
 */
export function popoverTransition(
  node: Element,
  params: { duration?: number; y?: number; startScale?: number } = {},
): TransitionConfig {
  const duration = motionDuration(params.duration ?? 120);
  const y = params.y ?? -6;
  const startScale = params.startScale ?? 0.98;
  const style = getComputedStyle(node);
  const baseOpacity = Number(style.opacity) || 1;
  const scaleDelta = 1 - startScale;

  return {
    duration,
    easing: cubicOut,
    css: (t, u) =>
      `opacity: ${t * baseOpacity}; translate: 0 ${(u * y).toFixed(2)}px; scale: ${(startScale + scaleDelta * t).toFixed(4)};`,
  };
}
