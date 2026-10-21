import { Tiktoken } from 'js-tiktoken/lite';
import o200kBase from 'js-tiktoken/ranks/o200k_base';

const encoder = new Tiktoken(o200kBase);
export const instructions = 'You rename minified JavaScript identifiers. Reply only with JSON.';
export const countTokens = text => encoder.encode(text, [], []).length;
const instructionTokens = countTokens(instructions);

export function promptText({ context, placeholders }) {
  return `Name placeholders: ${Object.keys(placeholders).join(', ')}. Use camelCase; PascalCase classes. No same scope duplicates. is/has/should/can prefixes only for boolean variables or functions returning boolean values.\n\n${context}`;
}

export const estimateInputTokens = payload => instructionTokens + countTokens(promptText(payload));
