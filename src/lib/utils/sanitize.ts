/**
 * Lightweight HTML sanitizer for rendering API content (e.g. Qobuz playlist descriptions).
 * Allows safe tags and strips dangerous elements/attributes.
 */

const ALLOWED_TAGS = new Set([
  'b', 'i', 'em', 'strong', 'br', 'a', 'p', 'span', 'ul', 'ol', 'li',
]);

/** Strip event handler attributes and javascript: URLs */
function stripDangerousAttrs(tag: string): string {
  // Remove on* event attributes
  tag = tag.replace(/\s+on\w+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]*)/gi, '');
  // Remove javascript: URLs from href/src
  tag = tag.replace(/(href|src)\s*=\s*(?:"javascript:[^"]*"|'javascript:[^']*')/gi, '$1=""');
  return tag;
}

export function sanitizeHtml(html: string): string {
  if (!html) return '';

  // Remove <script> and <iframe> blocks entirely (with content)
  let result = html.replace(/<script[\s\S]*?<\/script>/gi, '');
  result = result.replace(/<iframe[\s\S]*?<\/iframe>/gi, '');
  result = result.replace(/<style[\s\S]*?<\/style>/gi, '');

  // Process remaining tags: keep allowed, strip others
  result = result.replace(/<\/?([a-zA-Z][a-zA-Z0-9]*)\b[^>]*\/?>/g, (match, tagName) => {
    const lower = tagName.toLowerCase();
    if (ALLOWED_TAGS.has(lower)) {
      return stripDangerousAttrs(match);
    }
    return '';
  });

  return result;
}
