/** The desktop platform the UI runs on, read from the browser. Phones and
 *  tablets read as 'other', which takes the same wording and chrome as Linux. */
export function uiPlatform(): 'mac' | 'win' | 'other' {
  // Node before 21 has no navigator, and every translated string asks.
  if (typeof navigator === 'undefined') return 'other';
  const p = (
    (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ||
    navigator.platform ||
    ''
  ).toLowerCase();
  return p.includes('mac') ? 'mac' : p.includes('win') ? 'win' : 'other';
}
