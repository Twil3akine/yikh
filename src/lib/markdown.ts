import { Marked, Renderer } from 'marked';

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  })[character]!);
}

function safeUrl(value: string): string | null {
  const href = value.trim();
  if (!href || /[\u0000-\u0020\u007f\\]/.test(href)) return null;
  if (/^(?:https?:|mailto:)/i.test(href) || href.startsWith('#')) return href;
  return null;
}

const renderer = new Renderer();
renderer.html = ({ text }) => escapeHtml(text);
renderer.text = function (token) {
  if ('tokens' in token && token.tokens) return this.parser.parseInline(token.tokens);
  return escapeHtml(token.text).replace(/&amp;((?:#[0-9]+|#x[0-9a-f]+|[a-z][a-z0-9]*);)/gi, '&$1');
};
renderer.link = function ({ href, title, tokens }) {
  const body = this.parser.parseInline(tokens);
  const safe = safeUrl(href);
  if (!safe) return body;
  const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
  return `<a href="${escapeHtml(safe)}"${titleAttr} rel="noreferrer">${body}</a>`;
};
renderer.image = ({ text }) => escapeHtml(text);
renderer.code = ({ text }) => `<pre><code>${escapeHtml(text)}</code></pre>\n`;
renderer.codespan = ({ text }) => `<code>${escapeHtml(text)}</code>`;

const markdown = new Marked({ gfm: true, breaks: false, renderer });

export function renderMarkdown(source: string): string {
  return markdown.parse(source, { async: false }) as string;
}
