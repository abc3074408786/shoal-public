---
schema: design-pipeline.motion-foundation.v0.1
name: Velella control motion
posture: static
primitiveRegistry: design-pipeline.motion-primitives.v1
---
# Velella motion foundations
## Motion thesis
New settings controls use static state changes. Existing application transitions remain in motion.css without modification. Task status is immediate.
## Motion principles
Keep terminals and execution state stationary. Never delay a warning or permission.
## Motion vocabulary
Reuse existing CSS control response and dialog transitions in desktop/src/motion.css.
No new motion primitives are introduced by the task sandbox controls.
## Procedural motion
None. No simulation, autonomous animation, timeline or continuous rendering loop.
## Runtime policy
CSS owns existing transitions. React owns state. No animation library is introduced.
## Reduced motion
prefers-reduced-motion: reduce uses immediate state changes. Existing animations
are opt-in under no-preference. The fallback is an immediate state change. All information remains visible without motion.
## Source decisions
Preserve the existing desktop/src/motion.css. Task sandbox controls add no motion.

Adopted: existing reduced-motion behavior.
Rejected: additional motion in execution settings.
