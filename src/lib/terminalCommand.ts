import {
  isCompleteToolCallArguments,
  parseCompleteToolCallArgumentsObject
} from './parseToolCallArguments'

/**
 * Parse `command` from terminal tool `arguments` JSON (or raw fallback).
 *
 * The raw fallback is what keeps a half streamed payload readable: an incomplete
 * `arguments` string is shown as-is instead of being parsed (and thrown away).
 */
export function parseTerminalCommandFromArgs(argumentsJson: string): string {
  const text = argumentsJson?.trim()
  if (!text) return ''
  const args = parseCompleteToolCallArgumentsObject(text)
  if (!args) {
    // Half written (or not an object): keep the old "parse failed → raw" path.
    return isCompleteToolCallArguments(text) ? '' : text
  }
  return typeof args.command === 'string' ? args.command.trim() : ''
}
