import test from 'node:test';
import assert from 'node:assert/strict';
import factory from './fixtures/adapters.mjs';
import {kit} from '../scripts/adapter.mjs';
test('adapter owns refresh token extraction', () => {
 const adapters = factory(kit);
 assert.equal(adapters[1].exportEnv.token({cookies:[{name:'auth-refresh-token',value:'R'}]}), 'R');
 assert.equal(adapters[0].exportEnv.token({local_storage:[{localStorage:[{name:'session:refresh_token',value:'"S"'}]}]}), 'S');
});
