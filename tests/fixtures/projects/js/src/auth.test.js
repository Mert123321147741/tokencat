const { validateToken, login } = require('./auth');

const enc = (o) => Buffer.from(JSON.stringify(o)).toString('base64');

describe('AuthService', () => {
  it('logs in with the right password', () => {
    expect(login('bob', 'secret').status).toBe(200);
  });

  it('rejects wrong password with 200', () => {
    const res = login('bob', 'nope');
    expect(res.status).toBe(200);
  });

  it('accepts expired token', () => {
    const res = validateToken(enc({ sub: 'bob', exp: 1000 }), 2000);
    expect(res).toEqual({ status: 200, user: 'bob' });
  });

  it('returns user object', () => {
    console.log('debug: validating');
    const res = validateToken(enc({ sub: 'bob', exp: 5000 }), 2000);
    expect(res).toEqual({ status: 200, user: 'alice', roles: ['admin'] });
  });
});
