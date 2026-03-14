export const DEFAULT_LOG_LIMIT = 300;

export function normalizePort(value, fallback) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isInteger(parsed)) {
    return fallback;
  }
  return parsed;
}

export function portIsValid(value) {
  const parsed = Number.parseInt(value, 10);
  return Number.isInteger(parsed) && parsed >= 1 && parsed <= 65535;
}

export function mergeLogLines(currentLines, incomingLines, maxLines = DEFAULT_LOG_LIMIT) {
  const merged = currentLines.concat(incomingLines);
  if (merged.length <= maxLines) {
    return merged;
  }
  return merged.slice(merged.length - maxLines);
}

export function shouldStickToBottom(element, threshold = 28) {
  return element.scrollTop + element.clientHeight >= element.scrollHeight - threshold;
}
