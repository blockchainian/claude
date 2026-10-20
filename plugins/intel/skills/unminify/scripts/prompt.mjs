import { Tiktoken } from 'js-tiktoken/lite';
import o200kBase from 'js-tiktoken/ranks/o200k_base';

const encoder = new Tiktoken(o200kBase);
export const instructions = 'You rename minified JavaScript identifiers. Reply only with JSON.';
export const countTokens = text => encoder.encode(text, [], []).length;
const instructionTokens = countTokens(instructions);

export function promptText({ context, placeholders }) {
  return `Give descriptive names to these JavaScript placeholders: ${Object.keys(placeholders).join(', ')}. Use camelCase for variables and functions, and PascalCase for classes. Return a JSON object mapping every placeholder to its name.\n\n${context}`;
}

export const estimateInputTokens = payload => instructionTokens + countTokens(promptText(payload));
