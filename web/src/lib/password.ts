/**
 * The server's password policy (docs/06 §3.6) as far as the browser can check it: length and no
 * email name. The server also rejects very common passwords and says so.
 */
export function passwordProblem(password: string, email = ''): string | null {
  const n = [...password].length;
  if (n < 12) return 'Use at least 12 characters.';
  if (n > 128) return 'Use 128 characters or fewer.';
  const local = email.trim().toLowerCase().split('@')[0] ?? '';
  if (local.length >= 4 && password.toLowerCase().includes(local)) return 'Don’t include your email name in the password.';
  return null;
}

export const PASSWORD_HINT = 'At least 12 characters. A short sentence works well.';
