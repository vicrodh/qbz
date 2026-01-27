function escapeHtml(input: string): string {
  return input
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function renderInline(text: string): string {
  const escaped = escapeHtml(text);
  // Minimal inline formatting: **bold** and `code`
  const withBold = escaped.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  return withBold.replace(/`([^`]+)`/g, '<code>$1</code>');
}

function slugify(input: string): string {
  return input
    .toLowerCase()
    .trim()
    .replace(/[`*_~]/g, '')
    .replace(/[^a-z0-9\s-]/g, '')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-');
}

function countLeadingSpaces(line: string): number {
  let count = 0;
  for (const ch of line) {
    if (ch === ' ') count += 1;
    else if (ch === '\t') count += 2;
    else break;
  }
  return count;
}

export interface TocEntry {
  id: string;
  label: string;
  level: number;
}

export interface MarkdownRenderResult {
  html: string;
  toc: TocEntry[];
}

/**
 * Safe markdown renderer for release notes.
 *
 * This intentionally supports a small subset and escapes all HTML first,
 * which makes it safe to inject with {@html ...}.
 */
export function renderMarkdownWithToc(markdown: string | null | undefined): MarkdownRenderResult {
  if (!markdown) {
    return { html: '', toc: [] };
  }

  const lines = markdown.split('\n');
  const html: string[] = [];
  const toc: TocEntry[] = [];

  // Stack of list levels currently open (each entry is the level number)
  const openLists: number[] = [];

  const closeListsToLevel = (targetLevel: number) => {
    while (openLists.length > targetLevel) {
      html.push('</ul>');
      openLists.pop();
    }
  };

  const openListToLevel = (targetLevel: number) => {
    while (openLists.length < targetLevel) {
      html.push('<ul>');
      openLists.push(openLists.length + 1);
    }
  };

  const pushHeading = (label: string, level: number) => {
    const cleanLabel = label.trim();
    if (!cleanLabel) return;
    const id = slugify(cleanLabel);
    toc.push({ id, label: cleanLabel, level });
    // Use h3 for section headings to keep hierarchy subtle but visible.
    html.push(`<h3 id="${id}" class="wn-section">${renderInline(cleanLabel)}</h3>`);
  };

  for (const line of lines) {
    const trimmed = line.trim();

    if (!trimmed) {
      closeListsToLevel(0);
      continue;
    }

    // Headings (#, ##, ###)
    if (trimmed.startsWith('# ')) {
      closeListsToLevel(0);
      const label = trimmed.slice(2);
      pushHeading(label, 0);
      continue;
    }
    if (trimmed.startsWith('## ')) {
      closeListsToLevel(0);
      const label = trimmed.slice(3);
      pushHeading(label, 0);
      continue;
    }
    if (trimmed.startsWith('### ')) {
      closeListsToLevel(0);
      const label = trimmed.slice(4);
      pushHeading(label, 1);
      continue;
    }

    // List items with indentation-based nesting
    const isList = trimmed.startsWith('- ') || trimmed.startsWith('* ');
    if (isList) {
      const indentSpaces = countLeadingSpaces(line);
      const level = Math.floor(indentSpaces / 2);
      const content = trimmed.slice(2).trim();

      if (level === 0) {
        // Top-level bullets become section headings without bullets.
        closeListsToLevel(0);
        pushHeading(content, 0);
        continue;
      }

      openListToLevel(level);
      html.push(`<li>${renderInline(content)}</li>`);
      continue;
    }

    closeListsToLevel(0);
    html.push(`<p>${renderInline(trimmed)}</p>`);
  }

  closeListsToLevel(0);
  return { html: html.join('\n'), toc };
}

export function renderMarkdownSafe(markdown: string | null | undefined): string {
  return renderMarkdownWithToc(markdown).html;
}

export function formatReleaseDate(isoDate: string): string {
  const date = new Date(isoDate);
  if (Number.isNaN(date.getTime())) return isoDate;
  return new Intl.DateTimeFormat('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  }).format(date);
}
