export default kit => [{
  name: 'delta', domain: 'delta.example', startUrl: 'https://delta.example/', entryTexts: ['Login'],
  signIn: async () => {}, ready: async () => true,
  blockedHosts: ['data.delta.example'], blockedWebSockets: ['wss://data.delta.example/**'],
  byEmail: async ({cred}) => {
    const alias = kit.aliasFor(cred.email, 'delta');
    const hit = await kit.emailOtp.readSignupOtp(cred.email, cred.app_password, {
      toAlias: alias, sinceEpoch: 0, timeoutS: 0, pollS: 0,
    });
    if (!hit?.otp) return {status:'error', detail:'no signup code'};
    const proxyUrl = kit.config.proxyFor(alias);
    return kit.withProfile(alias, {proxyUrl}, async (_context, page) => {
      await page.fill('input[name=code]', hit.otp);
      return {status:'ok', alias, detail: await page.title(), proxyUrl, message: hit};
    });
  },
}];
