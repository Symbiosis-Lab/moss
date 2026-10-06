// Preview-click disambiguation.
//
// Single click = reveal (editor scrolls + flashes, focus stays in the preview).
// Double click = jump (editor focused, caret at the mapped position).
//
// Disambiguation is by CLICK COUNT (`event.detail`), not a delay timer, and
// that choice is deliberate: a 250–300ms timer would delay every single-click
// highlight perceptibly, and the vocabulary makes suppression unnecessary —
// the single action is now a strict prefix of the double (reveal ⊂ jump). So:
//
//   click  detail=1 → 'click'   (the reveal fires with zero latency; on a
//                                double it is simply superseded by the jump
//                                ~200ms later — the natural escalation, same
//                                as single-select → double-open in a file
//                                manager)
//   click  detail≥2 → null      (second click of a double: the reveal already
//                                fired once; dblclick owns the gesture)
//   dblclick        → 'dblclick'
//
// Pure and DOM-free so the state machine is directly unit-testable.

export type ClickInteraction = 'click' | 'dblclick' | null;

export function classifyClickGesture(
  eventType: 'click' | 'dblclick',
  detail: number,
): ClickInteraction {
  if (eventType === 'dblclick') return 'dblclick';
  return detail > 1 ? null : 'click';
}
