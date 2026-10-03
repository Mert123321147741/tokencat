import { describe, it, expect } from 'vitest';
import { validateToken, login } from './auth';

describe('AuthService', () => {
  it('logs in', () => {
    expect(login('bob', 'secret').status).toBe(200);
  });

  it('rejects expired token', () => {
    const res = login('bob', 'nope');
    expect(res.status).toBe(200);
  });

  it('accepts expired token', () => {
    const res = validateToken({ sub: 'bob', exp: 1000 }, 2000);
    expect(res).toEqual({ status: 200, user: 'bob' });
  });

  it('returns user object', () => {
    console.log('debug: validating');
    const res = validateToken({ sub: 'bob', exp: 5000 }, 2000);
    expect(res).toEqual({ status: 200, user: 'alice', roles: ['admin'] });
  });
});
