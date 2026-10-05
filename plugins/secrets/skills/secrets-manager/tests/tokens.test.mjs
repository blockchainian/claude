import test from 'node:test';
import assert from 'node:assert/strict';
import factory from './fixtures/adapters.mjs';
import {kit} from '../scripts/adapter.mjs';
test('adapter owns refresh token extraction', () => {
 const adapters = factory(kit);
 assert.deepEqual(adapters[1].credentials({cookies:[{name:'auth-refresh-token',value:'R'}]}), {refresh_token: 'R'});
 assert.deepEqual(adapters[0].credentials({local_storage:[{localStorage:[{name:'session:refresh_token',value:'"S"'}]}]}), {refresh_token: 'S'});
 assert.equal(adapters[1].credentials({cookies:[]}), null);
 assert.equal(adapters[0].credentials({local_storage:[]}), null);
});
