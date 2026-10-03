class UnauthorizedError extends Error {}

function decode(token) {
  return JSON.parse(Buffer.from(token, 'base64').toString());
}

function validateToken(token, now = Date.now()) {
  const payload = decode(token);
  if (payload.exp < now) {
    throw new UnauthorizedError(`token expired at ${payload.exp}`);
  }
  return { status: 200, user: payload.sub };
}

function login(user, password) {
  if (password !== 'secret') return { status: 401 };
  return { status: 200 };
}

module.exports = { validateToken, login, UnauthorizedError };
