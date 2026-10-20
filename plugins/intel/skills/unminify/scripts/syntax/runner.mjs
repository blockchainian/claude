// Ported from wakaru v1.14.0 rule pipeline (Apache-2.0).
import { parseSyntax, traverse, globalGuard } from './guards.mjs';
import { checkSyntaxRewrite } from './checks.mjs';
import { rules as defaults } from './registry.mjs';
import { fragmentFacts } from '../fragments.mjs';
export function transformSyntax(source, { rules = defaults, fragment = false, fragmentContext } = {}) {
  const steps = [];
  parseSyntax(source);
  for (const rule of rules) {
    const ast = parseSyntax(source), edits = [], allowedGlobals = new Set();
    traverse(ast,{Program(p){fragment ||= fragmentFacts(p).fragment;p.stop();}});
    const globalAllowed = globalGuard(ast,fragment,fragmentContext);
    const context = {
      source, fragment,
      isIntrinsicGlobal(path, name) {
        return globalAllowed(name, path.scope);
      },
      canIntroduceGlobal(path, name) {
        if (!rule.globals?.includes(name)) throw new Error(`undeclared intrinsic for ${rule.name}: ${name}`);
        if (!globalAllowed(name)) return false;
        allowedGlobals.add(name);
        return true;
      },
      replace(path, text) {
        const {start,end} = path.node ?? path;
        if (!Number.isInteger(start) || !Number.isInteger(end) || start<0 || end<start || end>source.length || typeof text!=='string') throw new Error('invalid syntax edit');
        // Keep compact keyword operands from merging into a different token.
        const wordEnd = /[\p{ID_Continue}$\u200c\u200d]$/u;
        const wordStart = /^[\p{ID_Continue}$\u200c\u200d]/u;
        if (wordEnd.test(source.slice(0,start)) && wordStart.test(text)) text = ` ${text}`;
        if (wordEnd.test(text) && wordStart.test(source.slice(end))) text += ' ';
        edits.push({start,end,text});
      },
    };
    traverse(ast,rule.visitor(context));
    edits.sort((a,b)=>a.start-b.start || a.end-b.end);
    let after='',offset=0;
    for(const edit of edits) {
      if(edit.start<offset) throw new Error(`overlapping syntax edits: ${rule.name}`);
      after+=source.slice(offset,edit.start)+edit.text;offset=edit.end;
    }
    after+=source.slice(offset);
    const reason = checkSyntaxRewrite(source,after,{fragment,fragmentContext,allowedGlobals:[...allowedGlobals]});
    if(reason) throw new Error(`${rule.name}: ${reason}`);
    steps.push({name:rule.name,edits});
    source=after;
  }
  return {source,steps};
}
