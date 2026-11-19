export function decimal(minor: number): string {
  const value = BigInt(Math.abs(minor));
  return `${value / 100n}.${String(value % 100n).padStart(2, '0')}`;
}

export function money(minor: number): string {
  const [whole, fraction] = decimal(minor).split('.');
  return `¥${Number(whole).toLocaleString()}.${fraction}`;
}

// Parse directly into integer fen, preserving the IPC safe-integer boundary.
export function parseMoney(value: string): number | null {
  const text = value.trim();
  if (!/^\d{1,14}(\.\d{1,2})?$/.test(text)) return null;
  const [whole, fraction = ''] = text.split('.');
  const minor = BigInt(whole) * 100n + BigInt(fraction.padEnd(2, '0'));
  if (minor <= 0n || minor > BigInt(Number.MAX_SAFE_INTEGER)) return null;
  return Number(minor);
}
