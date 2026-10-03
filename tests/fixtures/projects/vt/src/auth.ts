export class UnauthorizedError extends Error {}

export function validateToken(token: { sub: string; exp: number }, now = Date.now()) {
  if (token.exp < now) {
    throw new UnauthorizedError(`token expired at ${token.exp}`);
  }
  return { status: 200, user: token.sub };
}

export function login(user: string, password: string) {
  if (password !== 'secret') return { status: 401 };
  return { status: 200 };
}
