/**
 * Lightweight HTML sanitizer for rendering API content (e.g. Qobuz playlist descriptions).
 * Allows safe tags and strips dangerous elements/attributes.
 */

const ALLOWED_TAGS = new Set([
  'b', 'i', 'em', 'strong', 'br', 'a', 'p', 'span', 'ul', 'ol', 'li'
]);

const ALLOWED_ANCHOR_ATTRS = new Set(['href', 'title', 'target', 'rel']);
const SAFE_PROTOCOLS = new Set(['http:', 'https:', 'mailto:', 'tel:']);

function escapeHtml(input: string): string {
  return input
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function isSafeHref(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed) return false;
  if (trimmed.startsWith('/') || trimmed.startsWith('#')) return true;

  try {
    const parsed = new URL(trimmed, 'https://qbz.local');
    return SAFE_PROTOCOLS.has(parsed.protocol);
  } catch {
    return false;
  }
}

function sanitizeElement(element: Element): void {
  const children = Array.from(element.childNodes);

  for (const child of children) {
    if (child.nodeType === Node.COMMENT_NODE) {
      child.parentNode?.removeChild(child);
      continue;
    }

    if (child.nodeType !== Node.ELEMENT_NODE) {
      continue;
    }

    const childElement = child as Element;
    const tagName = childElement.tagName.toLowerCase();

    if (!ALLOWED_TAGS.has(tagName)) {
      const replacementNodes = Array.from(childElement.childNodes);
      for (const replacementNode of replacementNodes) {
        childElement.parentNode?.insertBefore(replacementNode, childElement);
        if (replacementNode.nodeType === Node.ELEMENT_NODE) {
          sanitizeElement(replacementNode as Element);
        }
      }
      childElement.parentNode?.removeChild(childElement);
      continue;
    }

    const attributes = Array.from(childElement.attributes);
    for (const attribute of attributes) {
      const attrName = attribute.name.toLowerCase();

      // Remove inline handlers and styles from all tags.
      if (attrName.startsWith('on') || attrName === 'style') {
        childElement.removeAttribute(attribute.name);
        continue;
      }

      if (tagName === 'a') {
        if (!ALLOWED_ANCHOR_ATTRS.has(attrName)) {
          childElement.removeAttribute(attribute.name);
          continue;
        }

        if (attrName === 'href' && !isSafeHref(attribute.value)) {
          childElement.removeAttribute(attribute.name);
        }
      } else {
        // Non-anchor tags keep no attributes.
        childElement.removeAttribute(attribute.name);
      }
    }

    if (tagName === 'a') {
      const target = childElement.getAttribute('target');
      if (target && target !== '_blank' && target !== '_self') {
        childElement.removeAttribute('target');
      }

      if (childElement.getAttribute('target') === '_blank') {
        childElement.setAttribute('rel', 'noopener noreferrer nofollow');
      }
    }

    sanitizeElement(childElement);
  }
}

export function sanitizeHtml(html: string): string {
  if (!html) return '';

  if (typeof window === 'undefined' || typeof DOMParser === 'undefined') {
    return escapeHtml(html);
  }

  const parser = new DOMParser();
  const doc = parser.parseFromString(`<div>${html}</div>`, 'text/html');
  const container = doc.body.firstElementChild;

  if (!container) {
    return '';
  }

  sanitizeElement(container);
  return container.innerHTML;
}
