// Ported from Wakaru v1.14.0 UnIndirectCall / eval_utils (Apache-2.0).
const unwrap = node => node?.type === 'ParenthesizedExpression' ? unwrap(node.expression) : node;

function memberName(node) {
  if (!node.computed && node.property.type === 'Identifier') return node.property.name;
  const property = unwrap(node.property);
  return node.computed && property.type === 'StringLiteral' ? property.value : null;
}

function propertyName(node) {
  if (node.computed) return null;
  return node.key.type === 'Identifier' ? node.key.name : node.key.type === 'StringLiteral' ? node.key.value : null;
}

function evalSource(node) {
  if (node.type === 'StringLiteral') return node.value;
  if (node.type !== 'CallExpression' || node.arguments.length !== 2) return null;
  const callee = node.callee;
  const [pattern, prefix] = node.arguments.map(arg => arg.type === 'SpreadElement' ? arg.argument : arg);
  return callee.type === 'MemberExpression' && !callee.computed && callee.property.name === 'replace'
    && callee.object.type === 'StringLiteral' && callee.object.value === 'quire'
    && pattern.type === 'RegExpLiteral' && pattern.pattern === '^'
    && prefix.type === 'StringLiteral' && prefix.value === 're' ? 'require' : null;
}

function observesReceiver(path) {
  let sensitive = false;
  function inspect(inner) {
    if (inner.isThisExpression() || (inner.isMemberExpression() && inner.node.object.type === 'Super')) sensitive = true;
    if (!inner.isCallExpression() || unwrap(inner.node.callee)?.type !== 'Identifier' || unwrap(inner.node.callee).name !== 'eval') return;
    const args = inner.node.arguments;
    if (!args.length) return;
    const source = args.some(arg => arg.type === 'SpreadElement') ? null : evalSource(args[0]);
    if (source === null || /(^|[^a-zA-Z0-9_$])this($|[^a-zA-Z0-9_$])/.test(source)) sensitive = true;
  }
  function scan(inner) {
    if (inner.isFunction() && !inner.isArrowFunctionExpression()) return;
    inspect(inner);
    inner.traverse(visitor);
  }
  const visitor = {
    Function(inner) {
      if (inner.isArrowFunctionExpression()) return;
      // Computed method keys run in the enclosing receiver's scope.
      if (inner.node.computed) scan(inner.get('key'));
      inner.skip();
    },
    ThisExpression: inspect,
    MemberExpression: inspect,
    CallExpression: inspect,
  };
  for (const child of [...path.get('params'), path.get('body')]) scan(child);
  return sensitive;
}

function receiverMembers(program) {
  const members = new Map();
  function add(object, name, scope) {
    const binding = scope.getBinding(object.name);
    if (!binding || name === null) return;
    if (!members.has(binding)) members.set(binding, new Set());
    members.get(binding).add(name);
  }
  function value(object, path, scope) {
    while (path.isParenthesizedExpression()) path = path.get('expression');
    if (path.isObjectExpression()) {
      for (const property of path.get('properties')) {
        if (property.isObjectMethod() && property.node.kind === 'method' && observesReceiver(property)) {
          add(object, propertyName(property.node), scope);
        } else if (property.isObjectProperty()) {
          let fn = property.get('value');
          while (fn.isParenthesizedExpression()) fn = fn.get('expression');
          if (fn.isFunctionExpression() && observesReceiver(fn)) add(object, propertyName(property.node), scope);
        }
      }
    } else if (path.isClass()) {
      for (const method of path.get('body.body')) {
        if (method.isClassMethod() && method.node.static && observesReceiver(method)) {
          add(object, propertyName(method.node), scope);
        }
      }
    }
  }
  program.traverse({
    VariableDeclarator(path) {
      if (path.node.id.type === 'Identifier' && path.node.init) value(path.node.id, path.get('init'), path.scope);
    },
    AssignmentExpression(path) {
      if (path.node.operator !== '=') return;
      const left = path.node.left;
      if (left.type === 'Identifier') value(left, path.get('right'), path.scope);
      else if (left.type === 'MemberExpression' && unwrap(left.object).type === 'Identifier') {
        let fn = path.get('right');
        while (fn.isParenthesizedExpression()) fn = fn.get('expression');
        if (fn.isFunctionExpression() && observesReceiver(fn)) add(unwrap(left.object), memberName(left), path.scope);
      }
    },
    ClassDeclaration(path) {
      if (path.node.id) value(path.node.id, path, path.parentPath.scope);
    },
  });
  return members;
}

function hasLocalMethod(binding, member) {
  if (!binding || binding.scope.path.isProgram() || !binding.constant) return false;
  const value = binding.path.isVariableDeclarator() ? unwrap(binding.path.node.init) : binding.path.node;
  if (!value) return false;
  const name = memberName(member);
  if (name === null) return false;
  if (value.type === 'ObjectExpression') {
    if (value.properties.some(property => property.type === 'SpreadElement' || property.computed)) return false;
    const property = value.properties.filter(property => propertyName(property) === name).at(-1);
    return property?.type === 'ObjectMethod' && property.kind === 'method'
      || property?.type === 'ObjectProperty' && unwrap(property.value).type === 'FunctionExpression';
  }
  if (value.type === 'ClassDeclaration' || value.type === 'ClassExpression') {
    return value.body.body.some(method => method.type === 'ClassMethod' && method.static
      && method.kind === 'method' && propertyName(method) === name);
  }
  return false;
}

export default {
  name: 'UnIndirectCall',
  visitor(context) {
    let members;
    const edits = [];
    function render(node) {
      let text = '', offset = node.start;
      for (const edit of edits.filter(edit => edit.start >= node.start && edit.end <= node.end).sort((a, b) => a.start - b.start)) {
        text += context.source.slice(offset, edit.start) + edit.text;
        offset = edit.end;
      }
      return text + context.source.slice(offset, node.end);
    }
    function candidate(node, path) {
      node = unwrap(node);
      if (node.type === 'Identifier') return node.name !== 'eval' && !path.findParent(parent => parent.isWithStatement()) ? node : null;
      if (node.type !== 'MemberExpression') return null;
      const object = unwrap(node.object);
      const binding = object.type === 'Identifier' ? path.scope.getBinding(object.name) : null;
      // Other slices may define or assign program-level members. Complete local
      // function bindings can still use the same receiver proof as whole modules.
      if (context.fragment && !hasLocalMethod(binding, node)) return null;
      return binding && members.get(binding)?.has(memberName(node)) ? null : node;
    }
    return {
      Program: {
        enter(path) { members = receiverMembers(path); },
        exit() { for (const edit of edits) context.replace(edit, edit.text); },
      },
      CallExpression: {
        exit(path) {
          const callee = path.node.callee, expression = unwrap(callee);
          let inner;
          if (expression.type === 'SequenceExpression') {
            const [zero, value] = expression.expressions;
            if (expression.expressions.length === 2 && zero.type === 'NumericLiteral' && zero.value === 0) inner = candidate(value, path);
          } else if (expression.type === 'CallExpression' && !context.fragment) {
            const object = unwrap(expression.callee);
            if (object.type === 'Identifier' && object.name === 'Object' && !path.scope.getBinding('Object')
              && expression.arguments.length === 1 && expression.arguments[0].type !== 'SpreadElement') {
              inner = candidate(expression.arguments[0], path);
            }
          }
          if (!inner) return;
          let text = render(inner);
          const statement = path.findParent(parent => parent.isExpressionStatement());
          if (statement?.node.expression.start === path.node.start && /^(?:\{|function\b|class\b|async\b|let\b)/.test(text)) text = `(${text})`;
          // Compose nested callee changes before emitting nonoverlapping edits.
          for (let index = edits.length - 1; index >= 0; index--) {
            if (edits[index].start >= callee.start && edits[index].end <= callee.end) edits.splice(index, 1);
          }
          edits.push({ start: callee.start, end: callee.end, text });
        },
      },
    };
  },
};
