export default kit => [
  {
    name: 'alpha', domain: 'alpha.example', startUrl: 'https://alpha.example/', entryTexts: ['Login'],
    signIn: async page => { await kit.clickFirst(page, ['Login']); await kit.clickFirst(page, ['Continue with Google']); },
    ready: page => kit.hasLsKey(page, 'session:token'),
    signedInUrl: url => { try { return new URL(url).pathname.startsWith('/token'); } catch { return false; } },
    verify: async ({ session }) => session.cookies.some(c => c.name === 'banned') ? 'restricted' : 'active',
    exportEnv: { envVar: 'ALPHA_REFRESH_TOKEN', token: session => {
      const value = session.local_storage.flatMap(o => o.localStorage ?? []).find(k => k.name === 'session:refresh_token')?.value;
      return value == null ? null : kit.restriction.unwrapJsonQuoted(value);
    } },
    blockedHosts: ['app-actions*.alpha.example', 'data.alpha.example'],
    blockedWebSockets: ['wss://data.alpha.example/**'],
  },
  {
    name: 'beta', domain: 'beta.example', startUrl: 'https://beta.example/', entryTexts: ['Sign up'], attempts: 3,
    signIn: async page => { await kit.clickFirst(page, ['Sign up']); await page.locator('iframe[src*="accounts.google.com/gsi/button"]').first().waitFor({ state: 'attached' }); await kit.clickFirst(page, ['Continue with Google']); },
    ready: page => kit.hasCookie(page, 'auth-access-token'),
    byEmail: async ({ cred, io }) => { const alias = kit.aliasFor(cred.email, 'beta'); io.log('fixture byEmail'); return { status: 'ok', alias }; },
    verify: async () => 'expired',
    exportEnv: { envVar: 'BETA_REFRESH_TOKEN', token: session => session.cookies.find(c => c.name === 'auth-refresh-token')?.value ?? null },
    blockedHosts: ['cluster*.beta.trade', 'pulse*.beta.trade', 'friends.beta.trade', 'telemetry.beta.trade', 'beta-assets-v2.beta-cdn.io', 'app-actions*.alpha.family', 'mobula-api.alpha.family'],
  },
];
